//! Rust engine integration with the shared signed-operation shell.
use super::State;
use crate::{Error, Result};
use generated_contracts::{AccountId32, OperationReceipt, UnixSeconds};
pub(crate) use provider_execution::VerifiedCall;
use provider_execution::{Admission, PreparedExecution};
pub(super) use provider_execution::{ExecutionInputs, Shell};
use rand::{Rng, RngExt, SeedableRng, rngs::StdRng};
use std::cell::RefCell;

pub(super) struct EngineContext {
    pub(super) root_account: AccountId32,
    pub(super) event_cursor: u64,
    random: RefCell<StdRng>,
}
// The RNG is execution-local. Every transition seeds its clone before use;
// checkpoints/snapshots carry only the business state and execution inputs.
impl Clone for EngineContext {
    fn clone(&self) -> Self {
        let mut context = Self::new(self.root_account);
        context.event_cursor = self.event_cursor;
        context
    }
}
impl Default for EngineContext {
    fn default() -> Self {
        Self::new(AccountId32::from_bytes([0; 32]))
    }
}
impl EngineContext {
    pub(super) fn new(root_account: AccountId32) -> Self {
        Self {
            root_account,
            event_cursor: 0,
            random: RefCell::new(StdRng::from_seed([0; 32])),
        }
    }
    fn reset(&mut self, inputs: &ExecutionInputs) {
        self.random = RefCell::new(StdRng::from_seed(inputs.seed));
    }
    pub(super) fn random_id(&self) -> generated_contracts::EntityId {
        let mut bytes = [0; 16];
        self.random.borrow_mut().fill_bytes(&mut bytes);
        generated_contracts::EntityId::from_bytes(bytes)
    }
    pub(super) fn choose(&self, length: usize) -> usize {
        self.random.borrow_mut().random_range(0..length)
    }
}
impl State {
    pub(crate) fn execute(
        self,
        verified: VerifiedCall,
        now: UnixSeconds,
    ) -> Result<(Self, OperationReceipt)> {
        match self.shell.prepare(verified, now)? {
            Admission::Replay(receipt) => Ok((self, receipt)),
            Admission::Execute(pending) => self.execute_prepared(*pending),
        }
    }
    #[cfg(test)]
    fn execute_with_inputs(
        self,
        verified: VerifiedCall,
        inputs: ExecutionInputs,
    ) -> Result<(Self, OperationReceipt)> {
        match self.shell.prepare_with_inputs(verified, inputs)? {
            Admission::Replay(receipt) => Ok((self, receipt)),
            Admission::Execute(pending) => self.execute_prepared(*pending),
        }
    }
    fn execute_prepared(mut self, pending: PreparedExecution) -> Result<(Self, OperationReceipt)> {
        let mut tentative = self.engine.clone();
        tentative.context.reset(pending.inputs());
        let call = pending.call();
        let transition = tentative.apply(call.origin, &call.command, pending.inputs().now);
        let transition = match transition {
            Ok(effect) => {
                tentative.validate()?;
                Ok(effect)
            }
            Err(error) if error.status == axum::http::StatusCode::SERVICE_UNAVAILABLE => {
                return Err(error);
            }
            Err(error) => Err(error.code()),
        };
        let success = transition.is_ok();
        let receipt = self.shell.complete(pending, transition)?;
        if success {
            tentative.context.event_cursor =
                receipt.last_event_cursor.ok_or_else(Error::internal)?;
            self.engine = tentative;
        }
        Ok((self, receipt))
    }
    #[cfg(feature = "storage-sqlite")]
    pub(super) fn validate_history(&self) -> Result<()> {
        self.shell.validate(self.shell.info.root_account)?;
        Ok(())
    }
}

#[cfg(all(test, feature = "mock-seed"))]
#[path = "directory_parity.rs"]
mod directory_parity;

#[cfg(all(test, feature = "mock-seed"))]
#[path = "matching_parity.rs"]
mod matching_parity;

#[cfg(all(test, feature = "mock-seed"))]
#[path = "project_parity.rs"]
mod project_parity;

#[cfg(all(test, feature = "mock-seed"))]
#[path = "financial_parity.rs"]
mod financial_parity;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::require;
    use generated_contracts::{OperationId, ProviderCommand, UnsignedContractCallV1};
    use subxt_signer::sr25519;
    fn key() -> Result<sr25519::Keypair> {
        sr25519::Keypair::from_secret_key([1; 32]).map_err(|_| Error::internal())
    }
    fn root() -> Result<AccountId32> {
        Ok(AccountId32::from_bytes(key()?.public_key().0))
    }
    use generated_contracts::{
        CalendarDefinition, CreateProjectRequest, Minutes, Qualifications, RegisterWorkerRequest,
        WorkerMode,
    };

    fn worker_command() -> ProviderCommand {
        ProviderCommand::RegisterWorker(RegisterWorkerRequest {
            display_name: "Candidate".into(),
            qualifications: Qualifications {
                role_ids: Vec::new(),
                skill_ids: Vec::new(),
            },
            calendar: CalendarDefinition {
                default_weekly_minutes: Minutes::new(600),
                overrides: Vec::new(),
            },
        })
    }

    fn call(state: &State, origin: AccountId32, command: ProviderCommand) -> Result<VerifiedCall> {
        let call = UnsignedContractCallV1::new(
            state.info().provider_instance_id,
            OperationId::from_bytes([9; 16]),
            origin,
            0,
            UnixSeconds::new(200),
            command,
        )?;
        let key = key()?;
        require(
            origin == AccountId32::from_bytes(key.public_key().0),
            "invalid_test_origin",
        )?;
        let signature =
            generated_contracts::Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0);
        Ok(VerifiedCall::verify(
            generated_contracts::SignedContractCallV1 { call, signature },
        )?)
    }

    #[test]
    fn identical_inputs_reproduce_generated_ids_and_exact_tie_selection() -> Result<()> {
        let root = root()?;
        let mut before = State::new(root)?;
        for value in [2, 3] {
            let account = AccountId32::from_bytes([value; 32]);
            before
                .engine
                .apply(account, &worker_command(), UnixSeconds::new(100))?;
            let worker = before.engine.worker_mut(account)?;
            worker.mode = WorkerMode::Coordinator;
            worker.coordinator_eligible = true;
        }
        let command = ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Reproducible".into(),
            description: String::new(),
        });
        let inputs = ExecutionInputs {
            version: 1,
            now: UnixSeconds::new(100),
            seed: [7; 32],
        };
        let (first, first_receipt) = before
            .clone()
            .execute_with_inputs(call(&before, root, command.clone())?, inputs.clone())?;
        let (second, second_receipt) = before
            .clone()
            .execute_with_inputs(call(&before, root, command.clone())?, inputs.clone())?;
        assert_eq!(first_receipt, second_receipt);
        assert_eq!(
            serde_json::to_value(&first.engine).map_err(|_| Error::internal())?,
            serde_json::to_value(&second.engine).map_err(|_| Error::internal())?
        );
        assert_eq!(first.shell.events, second.shell.events);
        let (other, other_receipt) = before.clone().execute_with_inputs(
            call(&before, root, command.clone())?,
            ExecutionInputs {
                seed: [8; 32],
                ..inputs
            },
        )?;
        assert_ne!(
            first_receipt.created_entity_id,
            other_receipt.created_entity_id
        );
        assert_eq!(other.shell.events.len(), 1);
        let retained_seed = first.shell.receipts[&first_receipt.operation_id]
            .inputs
            .as_ref()
            .map(|i| i.seed);
        let (replayed, receipt) =
            first.execute(call(&before, root, command)?, UnixSeconds::new(999))?;
        assert_eq!(receipt, first_receipt);
        assert_eq!(
            replayed.shell.receipts[&receipt.operation_id]
                .inputs
                .as_ref()
                .map(|i| i.seed),
            retained_seed
        );
        assert_eq!(replayed.shell.events.len(), 1);
        Ok(())
    }

    #[cfg(feature = "storage-sqlite")]
    #[test]
    fn legacy_checkpoint_keeps_generation_receipts_and_nonce_without_inputs() -> Result<()> {
        let root = root()?;
        let initial = State::new(root)?;
        let verified = call(&initial, root, worker_command())?;
        let replay = verified.clone();
        let (state, receipt) = initial.execute(verified, UnixSeconds::new(100))?;
        let decoded = serde_json::from_slice::<State>(&state.encode()?);
        assert!(
            decoded.is_ok(),
            "{}",
            decoded.err().map_or_else(String::new, |e| e.to_string())
        );
        let mut legacy = serde_json::to_value(&state).map_err(|_| Error::internal())?;
        legacy
            .as_object_mut()
            .ok_or_else(Error::internal)?
            .remove("engine_version");
        for recorded in legacy["receipts"]
            .as_object_mut()
            .ok_or_else(Error::internal)?
            .values_mut()
        {
            recorded
                .as_object_mut()
                .ok_or_else(Error::internal)?
                .remove("inputs");
        }
        assert!(legacy.get("shell").is_none());
        assert!(legacy.get("engine").is_none());
        let bytes = serde_json::to_vec(&legacy).map_err(|_| Error::internal())?;
        let restored = State::restore(&bytes, root)?;
        assert_eq!(restored.info(), state.info());
        assert_eq!(restored.nonce(root), state.nonce(root));
        assert_eq!(restored.snapshot(), state.snapshot());
        assert_eq!(restored.events(0, 100), state.events(0, 100));
        let (replayed, same) = restored.execute(replay, UnixSeconds::new(999))?;
        assert_eq!(same, receipt);
        assert_eq!(replayed.shell.events.len(), 1);
        legacy["engine_version"] = serde_json::json!(99);
        assert!(
            State::restore(
                &serde_json::to_vec(&legacy).map_err(|_| Error::internal())?,
                root
            )
            .is_err()
        );
        Ok(())
    }
    #[cfg(feature = "storage-sqlite")]
    #[test]
    fn checkpoint_keeps_scores_larger_than_a_json_u64_exact() -> Result<()> {
        let root = root()?;
        let mut state = State::new(root)?;
        state
            .engine
            .apply(root, &worker_command(), UnixSeconds::new(100))?;
        let score = generated_contracts::ReputationView {
            weighted_score_sum: u128::from(u64::MAX) * 499,
            rated_minutes: u64::MAX,
        };
        state.engine.worker_mut(root)?.worker_score = score.clone();
        let restored = State::restore(&state.encode()?, root)?;
        assert_eq!(restored.engine.workers[&root].worker_score, score);
        Ok(())
    }
}
