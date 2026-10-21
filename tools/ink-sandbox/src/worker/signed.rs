//! Explicit fixture bridge, not whole-domain coverage or an authority mode.
use super::*;
use generated_contracts::{DomainEventKind, EntityId, ProviderCommand};
use provider_execution::{Admission, Effect, VerifiedCall};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub struct FixtureWorker {
    pub account: AccountId32,
    pub calendar_id: EntityId,
}
/// Explicit genesis only. Whole-hour SetCalendar with no overrides is supported;
/// all other commands report incomplete coverage without consuming their nonce.
/// This is not a current-app calendar parity claim.
pub struct CalendarFixture {
    pub root: AccountId32,
    pub funded_accounts: Vec<AccountId32>,
    pub workers: Vec<FixtureWorker>,
    pub gas_limit: Weight,
}
impl CalendarFixture {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.funded_accounts.len() > 127
            || self.workers.len() > 127
            || !self.gas_limit.all_lte(GAS)
        {
            return Err(Error::Limit);
        }
        let accounts: BTreeSet<_> = self
            .funded_accounts
            .iter()
            .copied()
            .chain([self.root])
            .collect();
        let workers: BTreeSet<_> = self.workers.iter().map(|w| w.account).collect();
        let ids: BTreeSet<_> = self.workers.iter().map(|w| w.calendar_id).collect();
        if workers.len() != self.workers.len()
            || ids.len() != self.workers.len()
            || !workers.is_subset(&accounts)
        {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureState {
    version: u32,
    shell: Shell,
    calendar: Account,
    workers: BTreeMap<AccountId32, EntityId>,
}
fn runtime_account(account: AccountId32) -> Account {
    Account::new(account.into_bytes())
}
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut input = sp_core::hashing::blake2_256(name.as_bytes())[..4].to_vec();
    input.extend(args.encode());
    input
}
fn invocation(
    caller: AccountId32,
    action: Action,
    name: &str,
    args: impl Encode,
    gas: Weight,
    time: u64,
) -> Invocation {
    Invocation {
        caller: runtime_account(caller),
        action,
        input: input(name, args),
        gas,
        timestamp_ms: time,
    }
}
fn unit(outcome: &Outcome) -> Result<std::result::Result<(), &'static str>, Error> {
    match (&outcome.status, outcome.data.as_slice()) {
        (Status::Success, [0, 0]) => Ok(Ok(())),
        (Status::Reverted, [0, 1, 0]) => Ok(Err("worker_not_found")),
        (Status::LanguageError, [1, 1]) => Ok(Err("sandbox_language_error")),
        (Status::Dispatch(error), [])
            if *error == pallet_contracts::Error::<crate::Runtime>::OutOfGas.into() =>
        {
            Ok(Err("sandbox_out_of_gas"))
        }
        (Status::Dispatch(error), [])
            if *error == pallet_contracts::Error::<crate::Runtime>::ContractTrapped.into() =>
        {
            Ok(Err("sandbox_contract_trapped"))
        }
        _ => Err(Error::InvalidState),
    }
}
pub(super) fn initialize(
    bundles: &Bundles,
    fixture: CalendarFixture,
) -> Result<(sp_io::TestExternalities, Vec<u8>), Error> {
    fixture.validate()?;
    let accounts: BTreeSet<_> = fixture
        .funded_accounts
        .into_iter()
        .chain([fixture.root])
        .collect();
    let mut ext = new_externalities(accounts.into_iter().map(runtime_account).collect())?;
    let deployed = run(
        &mut ext,
        bundles,
        invocation(
            fixture.root,
            Action::Instantiate {
                bundle: "calendar".into(),
                salt: b"signed-calendar-v1".to_vec(),
            },
            "new",
            Option::<Account>::None,
            GAS,
            1_700_000_000_000,
        ),
    )?;
    unit(&deployed)?.map_err(|_| Error::InvalidState)?;
    let calendar = deployed.address.ok_or(Error::InvalidState)?;
    for worker in &fixture.workers {
        let outcome = run(
            &mut ext,
            bundles,
            invocation(
                fixture.root,
                Action::Call {
                    address: calendar.clone(),
                },
                "register_worker",
                runtime_account(worker.account),
                GAS,
                1_700_000_000_000,
            ),
        )?;
        unit(&outcome)?.map_err(|_| Error::InvalidState)?;
    }
    let state = FixtureState {
        version: 1,
        shell: Shell::new(fixture.root)?,
        calendar,
        workers: fixture
            .workers
            .into_iter()
            .map(|w| (w.account, w.calendar_id))
            .collect(),
    };
    Ok((ext, encode(&state)?))
}
fn encode(state: &FixtureState) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(state).map_err(|_| Error::InvalidState)?;
    if bytes.len() > MAX_SHELL {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
fn decode(stored: &Stored, root: AccountId32) -> Result<FixtureState, Error> {
    let state: FixtureState =
        serde_json::from_slice(&stored.shell).map_err(|_| Error::InvalidState)?;
    if state.version != 1 {
        return Err(Error::InvalidState);
    }
    state.shell.validate(root)?;
    Ok(state)
}
pub(super) fn validate(stored: &Stored, bundles: &Bundles, root: AccountId32) -> Result<(), Error> {
    let state = decode(stored, root)?;
    let mut ext = stored.checkpoint.clone().restore()?;
    let code = ext.execute_with(|| Contracts::code_hash(&state.calendar));
    if code
        != Some(H256::from(sp_core::hashing::blake2_256(
            bundles.code("calendar")?,
        )))
    {
        return Err(Error::InvalidState);
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
    let Mode::Calendar { root, gas } = mode else {
        return Err(Error::InvalidState);
    };
    // Signature verification happens before acquiring the writer transaction,
    // and executes on this dedicated worker, never a Tokio core thread.
    let verified = VerifiedCall::verify(call)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    let mut state = decode(&stored, root)?;
    let pending = match state.shell.prepare(verified, now)? {
        Admission::Replay(receipt) => {
            tx.rollback().await?;
            return Ok(receipt);
        }
        Admission::Execute(pending) => *pending,
    };
    let call = pending.call();
    let ProviderCommand::SetCalendar(definition) = &call.command else {
        return Err(Error::IncompleteCoverage);
    };
    let minutes = definition.default_weekly_minutes.get();
    if !definition.overrides.is_empty() || !minutes.is_multiple_of(60) {
        return Err(Error::IncompleteCoverage);
    }
    let hours = u8::try_from(minutes / 60).map_err(|_| Error::IncompleteCoverage)?;
    let time = pending
        .inputs()
        .now
        .get()
        .checked_mul(1000)
        .ok_or(Error::Limit)?;
    let origin = call.origin;
    let mut ext = stored.checkpoint.clone().restore()?;
    // WeeklyHours is SCALE availability variant 3 in the pinned v5 ABI.
    let outcome = run(
        &mut ext,
        bundles,
        invocation(
            origin,
            Action::Call {
                address: state.calendar.clone(),
            },
            "set_availability",
            (3_u8, hours),
            gas,
            time,
        ),
    )?;
    let transition = match unit(&outcome)? {
        Ok(()) => {
            let id = state
                .workers
                .get(&origin)
                .copied()
                .ok_or(Error::InvalidState)?;
            Ok(Effect::new(
                DomainEventKind::CalendarUpdated,
                Some(id),
                origin,
            ))
        }
        Err(code) => Err(code),
    };
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
    let Mode::Calendar { root, .. } = mode else {
        return Err(Error::InvalidState);
    };
    let mut tx = pool.begin().await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    let state = decode(&stored, root)?;
    tx.rollback().await?;
    Ok(state.shell)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_error_layers_are_distinct_and_unknown_abi_is_infrastructure_failure() {
        for (status, data, expected) in [
            (Status::Reverted, vec![0, 1, 0], "worker_not_found"),
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
            assert_eq!(
                unit(&Outcome {
                    status,
                    data,
                    address: None,
                    gas_consumed: Weight::zero()
                })
                .unwrap(),
                Err(expected)
            );
        }
        assert!(
            unit(&Outcome {
                status: Status::Reverted,
                data: vec![0, 1, 1],
                address: None,
                gas_consumed: Weight::zero()
            })
            .is_err()
        );
        assert!(
            unit(&Outcome {
                status: Status::Success,
                data: vec![0, 0, 99],
                address: None,
                gas_consumed: Weight::zero()
            })
            .is_err()
        );
    }
}
