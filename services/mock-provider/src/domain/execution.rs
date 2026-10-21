//! Signed-envelope execution and transport history. Domain rules live in `RustEngine`.
use super::{RecordedCall, State};
use crate::{Error, Result, random_id, require};
use generated_contracts::{
    AccountId32, DomainEvent, ExecutionOutcome, OperationId, OperationReceipt, PAYLOAD_VERSION,
    ProviderCommand, ProviderInfo, ProviderInstanceId, SignedContractCallV1, UnixSeconds,
    UnsignedContractCallV1,
};
use rand::{Rng, RngExt, SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeMap;
use subxt_signer::sr25519;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Shell {
    pub(super) info: ProviderInfo,
    pub(super) nonces: BTreeMap<AccountId32, u64>,
    pub(super) receipts: BTreeMap<OperationId, RecordedCall>,
    pub(super) events: Vec<DomainEvent>,
}
impl Shell {
    pub(super) fn new(root_account: AccountId32) -> Result<Self> {
        Ok(Self {
            info: ProviderInfo {
                provider_instance_id: ProviderInstanceId::from_bytes(random_id()?.into_bytes()),
                root_account,
                payload_version: PAYLOAD_VERSION,
            },
            nonces: BTreeMap::new(),
            receipts: BTreeMap::new(),
            events: Vec::new(),
        })
    }
}

/// Retain one seed and time per accepted operation for differential replay.
/// Version 1 uses the rand 0.10.2 locked `StdRng` stream and unbiased range sampling.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ExecutionInputs {
    pub(super) version: u8,
    pub(super) now: UnixSeconds,
    seed: [u8; 32],
}
impl ExecutionInputs {
    fn capture(now: UnixSeconds) -> Result<Self> {
        let mut seed = [0; 32];
        getrandom::fill(&mut seed).map_err(|_| Error::internal())?;
        Ok(Self {
            version: 1,
            now,
            seed,
        })
    }
}

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
/// Typestate gate: unverified envelopes cannot be handed to the state engine.
pub(crate) struct VerifiedCall {
    pub(super) call: UnsignedContractCallV1,
    pub(super) bytes: Vec<u8>,
}

impl VerifiedCall {
    pub(crate) fn verify(signed: SignedContractCallV1) -> Result<Self> {
        let bytes = signed.call.signable_bytes()?;
        let public = sr25519::PublicKey(signed.call.origin.into_bytes());
        let signature = sr25519::Signature(signed.signature.into_bytes());
        require(
            sr25519::verify(&signature, &bytes, &public),
            "invalid_signature",
        )?;
        Ok(Self {
            call: signed.call,
            bytes,
        })
    }
}

impl State {
    /// Failed domain execution commits only its nonce and failed receipt; its
    /// tentative business state and events are discarded as a unit.
    pub(crate) fn execute(
        self,
        verified: VerifiedCall,
        now: UnixSeconds,
    ) -> Result<(Self, OperationReceipt)> {
        let VerifiedCall { call, bytes } = verified;
        require(
            call.provider_instance_id == self.shell.info.provider_instance_id,
            "provider_instance_mismatch",
        )?;
        if let Some(record) = self.shell.receipts.get(&call.operation_id) {
            require(record.bytes == bytes, "idempotency_conflict")?;
            let receipt = record.receipt.clone();
            return Ok((self, receipt));
        }
        require(call.expires_at > now, "expired_call")?;
        require(
            call.nonce == self.nonce(call.origin).nonce,
            "nonce_conflict",
        )?;
        let inputs = ExecutionInputs::capture(now)?;
        self.execute_with_inputs(VerifiedCall { call, bytes }, inputs)
    }

    fn execute_with_inputs(
        mut self,
        verified: VerifiedCall,
        inputs: ExecutionInputs,
    ) -> Result<(Self, OperationReceipt)> {
        let VerifiedCall { call, bytes } = verified;
        let now = inputs.now;
        let next_nonce = call
            .nonce
            .checked_add(1)
            .ok_or_else(|| Error::domain("nonce_exhausted"))?;
        let mut tentative = self.engine.clone();
        tentative.context.reset(&inputs);
        let transition = tentative.apply(call.origin, &call.command, now);
        let (outcome, entity_id, cursor) = match transition {
            Ok(effect) => {
                tentative.validate()?;
                let cursor = u64::try_from(self.shell.events.len())
                    .map_err(|_| Error::internal())?
                    .checked_add(1)
                    .ok_or_else(Error::internal)?;
                self.shell.events.push(DomainEvent {
                    provider_instance_id: self.shell.info.provider_instance_id,
                    cursor,
                    operation_id: call.operation_id,
                    kind: effect.kind,
                    project_id: call.command.project_id().or({
                        if matches!(call.command, ProviderCommand::CreateProject(_)) {
                            effect.entity_id
                        } else {
                            None
                        }
                    }),
                    entity_id: effect.entity_id,
                    recipients: effect.recipients.into_iter().collect(),
                    occurred_at: now,
                    origin: Some(call.origin),
                });
                tentative.context.event_cursor = cursor;
                self.engine = tentative;
                (ExecutionOutcome::Success, effect.entity_id, Some(cursor))
            }
            Err(error) if error.status == axum::http::StatusCode::SERVICE_UNAVAILABLE => {
                return Err(error);
            }
            Err(error) => (ExecutionOutcome::Failed(error.code().into()), None, None),
        };
        let receipt = OperationReceipt {
            operation_id: call.operation_id,
            provider_instance_id: self.shell.info.provider_instance_id,
            origin: call.origin,
            nonce: call.nonce,
            outcome,
            created_entity_id: entity_id,
            first_event_cursor: cursor,
            last_event_cursor: cursor,
            finalized_at: now,
        };
        self.shell.nonces.insert(call.origin, next_nonce);
        self.shell.receipts.insert(
            call.operation_id,
            RecordedCall {
                bytes,
                inputs: Some(inputs),
                receipt: receipt.clone(),
            },
        );
        Ok((self, receipt))
    }
    #[cfg(feature = "storage-sqlite")]
    pub(super) fn validate_history(&self) -> Result<()> {
        for (index, event) in self.shell.events.iter().enumerate() {
            require(
                event.provider_instance_id == self.shell.info.provider_instance_id
                    && event.cursor == u64::try_from(index).map_err(|_| Error::internal())? + 1,
                "invalid_event_state",
            )?;
        }
        for (operation_id, recorded) in &self.shell.receipts {
            if let Some(inputs) = &recorded.inputs {
                require(
                    inputs.version == 1 && inputs.now == recorded.receipt.finalized_at,
                    "invalid_execution_inputs",
                )?;
            }
            let call = UnsignedContractCallV1::decode_signable(&recorded.bytes)?;
            require(
                call.operation_id == *operation_id
                    && recorded.receipt.operation_id == *operation_id
                    && call.provider_instance_id == self.shell.info.provider_instance_id
                    && recorded.receipt.origin == call.origin
                    && recorded.receipt.nonce == call.nonce
                    && self.nonce(call.origin).nonce > call.nonce,
                "invalid_receipt_state",
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let bytes = call.signable_bytes()?;
        Ok(VerifiedCall { call, bytes })
    }

    #[test]
    fn identical_inputs_reproduce_generated_ids_and_exact_tie_selection() -> Result<()> {
        let root = AccountId32::from_bytes([1; 32]);
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
        let root = AccountId32::from_bytes([1; 32]);
        let initial = State::new(root)?;
        let verified = call(&initial, root, worker_command())?;
        let replay = VerifiedCall {
            call: verified.call.clone(),
            bytes: verified.bytes.clone(),
        };
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
        let root = AccountId32::from_bytes([1; 32]);
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
