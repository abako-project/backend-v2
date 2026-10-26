//! Shared signed-operation shell. Engines own business rules; storage commits
//! the returned shell and tentative engine state in the same transaction.
#![allow(clippy::missing_errors_doc)]

pub mod migration;

use generated_contracts::{
    AccountId32, DomainEvent, DomainEventKind, EntityId, ExecutionOutcome, OperationId,
    OperationReceipt, PAYLOAD_VERSION, ProviderCommand, ProviderInfo, ProviderInstanceId,
    SignedContractCallV1, UnixSeconds, UnsignedContractCallV1,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A stable sanitized error; no caller data or signature is retained in it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct Error {
    code: &'static str,
    infrastructure: bool,
}
impl Error {
    fn invalid(code: &'static str) -> Self {
        Self {
            code,
            infrastructure: false,
        }
    }
    fn unavailable() -> Self {
        Self {
            code: "provider_unavailable",
            infrastructure: true,
        }
    }
    /// Stable provider reason.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
    /// Infrastructure failures must never consume a nonce or record a receipt.
    #[must_use]
    pub const fn is_infrastructure(&self) -> bool {
        self.infrastructure
    }
}
impl From<generated_contracts::ContractError> for Error {
    fn from(_: generated_contracts::ContractError) -> Self {
        Self::invalid("invalid_contract")
    }
}
/// Shell errors never carry business rejection outcomes.
pub type Result<T> = std::result::Result<T, Error>;
fn require(condition: bool, code: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::invalid(code))
    }
}

/// Retained time and entropy. Version 1 uses the native locked rand 0.10.2
/// stream in the Rust engine; future portable contract entropy needs a new version.
#[derive(Clone, Serialize, Deserialize)]
pub struct ExecutionInputs {
    /// Input interpretation version.
    pub version: u8,
    /// One accepted execution time.
    pub now: UnixSeconds,
    /// One accepted execution seed, reused by comparison/replay tooling.
    pub seed: [u8; 32],
}
impl ExecutionInputs {
    fn capture(now: UnixSeconds) -> Result<Self> {
        let mut seed = [0; 32];
        getrandom::fill(&mut seed).map_err(|_| Error::unavailable())?;
        Ok(Self {
            version: 1,
            now,
            seed,
        })
    }
}
/// Durable history keeps the original signable bytes for exact replay matching.
#[derive(Clone, Serialize, Deserialize)]
pub struct RecordedCall {
    /// Exact versioned signed content, excluding its signature.
    pub bytes: Vec<u8>,
    /// Absent on historical version-1 records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<ExecutionInputs>,
    /// Finality is independent of success or domain rejection.
    pub receipt: OperationReceipt,
    /// Exact core allocation for compact task creation; never the public storage ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocated_task_id: Option<u32>,
}
/// Internal storage model shared by the two engines, never a browser DTO.
#[derive(Clone, Serialize, Deserialize)]
pub struct Shell {
    /// Generation, trusted root and payload version.
    pub info: ProviderInfo,
    /// Next nonce per origin.
    pub nonces: BTreeMap<AccountId32, u64>,
    /// Durable operation history.
    pub receipts: BTreeMap<OperationId, RecordedCall>,
    /// Durable ordered events.
    pub events: Vec<DomainEvent>,
}
/// An engine's successful domain event; no transport policy belongs in engines.
pub struct Effect {
    /// Public event kind.
    pub kind: DomainEventKind,
    /// Optional created/affected entity.
    pub entity_id: Option<EntityId>,
    /// Recipient identities; storage/adapter retain projection ownership.
    pub recipients: BTreeSet<AccountId32>,
    /// Private allocation committed atomically with a successful compact creation.
    pub allocated_task_id: Option<u32>,
}
impl Effect {
    /// Construct an effect with one recipient; engines may add others.
    #[must_use]
    pub fn new(kind: DomainEventKind, entity_id: Option<EntityId>, recipient: AccountId32) -> Self {
        Self {
            kind,
            entity_id,
            recipients: BTreeSet::from([recipient]),
            allocated_task_id: None,
        }
    }
}
/// A verified envelope. Fields stay private so callers cannot forge this gate.
#[derive(Clone)]
pub struct VerifiedCall {
    call: UnsignedContractCallV1,
    bytes: Vec<u8>,
}
impl VerifiedCall {
    /// Validate the existing wire contract and sr25519 signature off Tokio cores.
    pub fn verify(signed: SignedContractCallV1) -> Result<Self> {
        let bytes = signed.call.signable_bytes()?;
        // Same primitive and signing context used by subxt-signer::sr25519.
        // This verification-only boundary needs no mnemonic/key-derivation API.
        let signature = schnorrkel::Signature::from_bytes(&signed.signature.into_bytes())
            .map_err(|_| Error::invalid("invalid_signature"))?;
        let public = schnorrkel::PublicKey::from_bytes(&signed.call.origin.into_bytes())
            .map_err(|_| Error::invalid("invalid_signature"))?;
        require(
            public
                .verify_simple(b"substrate", &bytes, &signature)
                .is_ok(),
            "invalid_signature",
        )?;
        Ok(Self {
            call: signed.call,
            bytes,
        })
    }
}
/// Admission returns replay before expiry/nonce checks or fresh input capture.
pub enum Admission {
    /// Return the old receipt without invoking an engine or writing storage.
    Replay(OperationReceipt),
    /// Execute once against tentative business state.
    Execute(Box<PreparedExecution>),
}
/// Shell-owned inputs for exactly one engine execution.
pub struct PreparedExecution {
    verified: VerifiedCall,
    inputs: ExecutionInputs,
    cursor: u64,
    next_nonce: u64,
}
impl PreparedExecution {
    /// The verified caller and command are immutable to the engine.
    #[must_use]
    pub fn call(&self) -> &UnsignedContractCallV1 {
        &self.verified.call
    }
    /// Captured execution context, also retained with its receipt.
    #[must_use]
    pub fn inputs(&self) -> &ExecutionInputs {
        &self.inputs
    }
}
impl Shell {
    /// Create an empty generation. Opening existing data must restore it instead.
    pub fn new(root_account: AccountId32) -> Result<Self> {
        let mut id = [0; 16];
        getrandom::fill(&mut id).map_err(|_| Error::unavailable())?;
        Ok(Self {
            info: ProviderInfo {
                provider_instance_id: ProviderInstanceId::from_bytes(id),
                root_account,
                payload_version: PAYLOAD_VERSION,
            },
            nonces: BTreeMap::new(),
            receipts: BTreeMap::new(),
            events: Vec::new(),
        })
    }
    /// Next nonce for a new operation; authenticated replay ignores this value.
    #[must_use]
    pub fn nonce(&self, origin: AccountId32) -> u64 {
        self.nonces.get(&origin).copied().unwrap_or(0)
    }
    /// Recover the exact private task key from this operation's retained result.
    /// Neither live tasks nor the current allocator can identify a past allocation.
    pub fn task_allocation(
        &self,
        operation: OperationId,
    ) -> Result<Option<generated_contracts::provider_core::TaskAllocationV1>> {
        let Some(record) = self.receipts.get(&operation) else {
            return Ok(None);
        };
        let Some(task_id) = record.allocated_task_id else {
            return Ok(None);
        };
        let call = UnsignedContractCallV1::decode_signable(&record.bytes)
            .map_err(|_| Error::unavailable())?;
        let ProviderCommand::CreateTaskCore {
            project_id,
            task_storage_id,
            ..
        } = call.command
        else {
            return Err(Error::unavailable());
        };
        if task_id == 0
            || task_id == u32::MAX
            || record.receipt.outcome != ExecutionOutcome::Success
            || record.receipt.operation_id != operation
            || call.operation_id != operation
            || record.receipt.provider_instance_id != self.info.provider_instance_id
            || call.provider_instance_id != self.info.provider_instance_id
            || record.receipt.origin != call.origin
            || record.receipt.nonce != call.nonce
            || record.receipt.created_entity_id != Some(task_storage_id)
        {
            return Err(Error::unavailable());
        }
        Ok(Some(generated_contracts::provider_core::TaskAllocationV1 {
            receipt: record.receipt.clone(),
            project_id,
            task_storage_id,
            task_id,
        }))
    }
    fn check(&self, verified: &VerifiedCall, now: UnixSeconds) -> Result<Option<OperationReceipt>> {
        let call = &verified.call;
        require(
            call.provider_instance_id == self.info.provider_instance_id,
            "provider_instance_mismatch",
        )?;
        if let Some(record) = self.receipts.get(&call.operation_id) {
            require(record.bytes == verified.bytes, "idempotency_conflict")?;
            return Ok(Some(record.receipt.clone()));
        }
        // V5 retains exact replay but does not execute content-bearing task requests.
        // The adapter must retain that intent and submit a compact core command.
        if matches!(
            call.command,
            ProviderCommand::CreateTask { .. }
                | ProviderCommand::EditTask { .. }
                | ProviderCommand::UpdateTaskProgress { .. }
        ) {
            return Err(Error::unavailable());
        }
        require(call.expires_at > now, "expired_call")?;
        require(call.nonce == self.nonce(call.origin), "nonce_conflict")?;
        Ok(None)
    }
    /// Admit fresh execution using one newly captured time and seed.
    pub fn prepare(&self, verified: VerifiedCall, now: UnixSeconds) -> Result<Admission> {
        if let Some(receipt) = self.check(&verified, now)? {
            return Ok(Admission::Replay(receipt));
        }
        self.pending(verified, ExecutionInputs::capture(now)?)
    }
    /// Reuse captured inputs for differential execution; still validates the
    /// complete envelope before engine access. Never accepts entropy from HTTP.
    pub fn prepare_with_inputs(
        &self,
        verified: VerifiedCall,
        inputs: ExecutionInputs,
    ) -> Result<Admission> {
        if let Some(receipt) = self.check(&verified, inputs.now)? {
            return Ok(Admission::Replay(receipt));
        }
        require(inputs.version == 1, "invalid_execution_inputs")?;
        self.pending(verified, inputs)
    }
    fn pending(&self, verified: VerifiedCall, inputs: ExecutionInputs) -> Result<Admission> {
        let next_nonce = verified
            .call
            .nonce
            .checked_add(1)
            .ok_or_else(|| Error::invalid("nonce_exhausted"))?;
        let cursor = u64::try_from(self.events.len()).map_err(|_| Error::unavailable())?;
        Ok(Admission::Execute(Box::new(PreparedExecution {
            verified,
            inputs,
            cursor,
            next_nonce,
        })))
    }
    /// Finish a domain transition. On rejection storage must discard tentative
    /// business state; on infrastructure failure do not call this method.
    pub fn complete(
        &mut self,
        pending: PreparedExecution,
        transition: std::result::Result<Effect, &'static str>,
    ) -> Result<OperationReceipt> {
        let PreparedExecution {
            verified: VerifiedCall { call, bytes },
            inputs,
            cursor: old_cursor,
            next_nonce,
        } = pending;
        require(
            self.info.provider_instance_id == call.provider_instance_id
                && self.nonce(call.origin) == call.nonce
                && u64::try_from(self.events.len()).map_err(|_| Error::unavailable())?
                    == old_cursor
                && !self.receipts.contains_key(&call.operation_id),
            "stale_execution",
        )?;
        let (outcome, entity_id, cursor, allocated_task_id) = match transition {
            Ok(effect) => {
                let compact_create = matches!(call.command, ProviderCommand::CreateTaskCore { .. });
                if compact_create != effect.allocated_task_id.is_some()
                    || effect
                        .allocated_task_id
                        .is_some_and(|id| id == 0 || id == u32::MAX)
                {
                    return Err(Error::unavailable());
                }
                if let ProviderCommand::CreateTaskCore {
                    task_storage_id, ..
                } = &call.command
                    && (effect.entity_id != Some(*task_storage_id)
                        || effect.kind != DomainEventKind::TaskCreated)
                {
                    return Err(Error::unavailable());
                }
                let cursor = old_cursor.checked_add(1).ok_or_else(Error::unavailable)?;
                self.events.push(DomainEvent {
                    provider_instance_id: self.info.provider_instance_id,
                    cursor,
                    operation_id: call.operation_id,
                    kind: effect.kind,
                    project_id: call.command.project_id().or(
                        if matches!(
                            call.command,
                            ProviderCommand::CreateProject(_)
                                | ProviderCommand::CreateProjectCore(_)
                        ) {
                            effect.entity_id
                        } else {
                            None
                        },
                    ),
                    entity_id: effect.entity_id,
                    recipients: effect.recipients.into_iter().collect(),
                    occurred_at: inputs.now,
                    origin: Some(call.origin),
                });
                (
                    ExecutionOutcome::Success,
                    effect.entity_id,
                    Some(cursor),
                    effect.allocated_task_id,
                )
            }
            Err(code) => (ExecutionOutcome::Failed(code.into()), None, None, None),
        };
        let receipt = OperationReceipt {
            operation_id: call.operation_id,
            provider_instance_id: self.info.provider_instance_id,
            origin: call.origin,
            nonce: call.nonce,
            outcome,
            created_entity_id: entity_id,
            first_event_cursor: cursor,
            last_event_cursor: cursor,
            finalized_at: inputs.now,
        };
        self.nonces.insert(call.origin, next_nonce);
        self.receipts.insert(
            call.operation_id,
            RecordedCall {
                bytes,
                inputs: Some(inputs),
                receipt: receipt.clone(),
                allocated_task_id,
            },
        );
        Ok(receipt)
    }
    /// Validate persisted history without re-signing or replaying domain effects.
    pub fn validate(&self, root: AccountId32) -> Result<()> {
        require(
            self.info.root_account == root && self.info.payload_version == PAYLOAD_VERSION,
            "state_configuration_mismatch",
        )?;
        for (index, event) in self.events.iter().enumerate() {
            require(
                event.provider_instance_id == self.info.provider_instance_id
                    && event.cursor == u64::try_from(index).map_err(|_| Error::unavailable())? + 1,
                "invalid_event_state",
            )?;
        }
        let mut task_allocations = BTreeSet::new();
        for (id, recorded) in &self.receipts {
            if let Some(inputs) = &recorded.inputs {
                require(
                    inputs.version == 1 && inputs.now == recorded.receipt.finalized_at,
                    "invalid_execution_inputs",
                )?;
            }
            let call = UnsignedContractCallV1::decode_signable(&recorded.bytes)?;
            let created_task = matches!(call.command, ProviderCommand::CreateTaskCore { .. })
                && recorded.receipt.outcome == ExecutionOutcome::Success;
            require(
                if created_task {
                    recorded
                        .allocated_task_id
                        .is_some_and(|id| id != 0 && id != u32::MAX)
                } else {
                    recorded.allocated_task_id.is_none()
                },
                "invalid_task_allocation_state",
            )?;
            if created_task {
                let allocation = self.task_allocation(*id)?.ok_or_else(Error::unavailable)?;
                require(
                    task_allocations.insert(allocation.task_id),
                    "duplicate_task_allocation",
                )?;
            }
            require(
                call.operation_id == *id
                    && recorded.receipt.operation_id == *id
                    && recorded.receipt.provider_instance_id == self.info.provider_instance_id
                    && call.provider_instance_id == self.info.provider_instance_id
                    && recorded.receipt.origin == call.origin
                    && recorded.receipt.nonce == call.nonce
                    && self.nonce(call.origin) > call.nonce,
                "invalid_receipt_state",
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use generated_contracts::{CalendarDefinition, Minutes};
    fn call(shell: &Shell, id: u8) -> Result<VerifiedCall> {
        // Signature verification is independently covered with real Subxt/SP
        // signatures by the Rust-provider and WASM integration suites.
        let call = UnsignedContractCallV1::new(
            shell.info.provider_instance_id,
            OperationId::from_bytes([id; 16]),
            shell.info.root_account,
            0,
            UnixSeconds::new(200),
            ProviderCommand::SetCalendar(CalendarDefinition {
                default_weekly_minutes: Minutes::new(600),
                overrides: Vec::new(),
            }),
        )?;
        let bytes = call.signable_bytes()?;
        Ok(VerifiedCall { call, bytes })
    }
    fn prepared(shell: &Shell, call: VerifiedCall) -> Result<PreparedExecution> {
        match shell.prepare_with_inputs(
            call,
            ExecutionInputs {
                version: 1,
                now: UnixSeconds::new(100),
                seed: [7; 32],
            },
        )? {
            Admission::Execute(pending) => Ok(*pending),
            Admission::Replay(_) => Err(Error::invalid("unexpected_test_replay")),
        }
    }
    #[test]
    fn stale_completion_cannot_append_events_or_replace_another_receipt() -> Result<()> {
        let root = AccountId32::from_bytes([1; 32]);
        let mut shell = Shell::new(root)?;
        let first = prepared(&shell, call(&shell, 1)?)?;
        let stale = prepared(&shell, call(&shell, 2)?)?;
        shell.complete(
            first,
            Ok(Effect::new(DomainEventKind::CalendarUpdated, None, root)),
        )?;
        let error = shell
            .complete(
                stale,
                Ok(Effect::new(DomainEventKind::CalendarUpdated, None, root)),
            )
            .err()
            .ok_or_else(Error::unavailable)?;
        assert_eq!(error.code(), "stale_execution");
        assert_eq!(shell.nonce(root), 1);
        assert_eq!(shell.events.len(), 1);
        assert_eq!(shell.receipts.len(), 1);
        shell.validate(root)?;
        Ok(())
    }
    #[test]
    fn replay_reuses_inputs_and_validation_rejects_a_receipt_from_another_generation() -> Result<()>
    {
        let root = AccountId32::from_bytes([1; 32]);
        let mut shell = Shell::new(root)?;
        let verified = call(&shell, 1)?;
        let pending = prepared(&shell, verified.clone())?;
        let receipt = shell.complete(pending, Err("worker_not_found"))?;
        assert!(
            matches!(shell.prepare_with_inputs(verified,ExecutionInputs { version:99,now:UnixSeconds::new(999),seed:[9;32] })?,Admission::Replay(same) if same==receipt)
        );
        assert!(shell.events.is_empty());
        assert_eq!(shell.nonce(root), 1);
        let recorded = shell
            .receipts
            .get_mut(&receipt.operation_id)
            .ok_or_else(Error::unavailable)?;
        assert_eq!(recorded.inputs.as_ref().map(|i| i.seed), Some([7; 32]));
        recorded.receipt.provider_instance_id = ProviderInstanceId::from_bytes([9; 16]);
        assert_eq!(
            shell.validate(root).err().map(|e| e.code()),
            Some("invalid_receipt_state")
        );
        Ok(())
    }
    #[test]
    fn task_completion_records_allocation_atomically_and_replay_preserves_it()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let root = AccountId32::from_bytes([1; 32]);
        let mut shell = Shell::new(root)?;
        let mut verified = call(&shell, 1)?;
        verified.call.command = ProviderCommand::CreateTaskCore {
            project_id: EntityId::from_bytes([2; 16]),
            task_storage_id: EntityId::from_bytes([3; 16]),
            assignees: Vec::new(),
        };
        verified.bytes = verified.call.signable_bytes()?;
        let before = (
            shell.receipts.len(),
            shell.events.len(),
            shell.nonce(verified.call.origin),
        );
        let pending = prepared(&shell, verified.clone())?;
        let mut effect = Effect::new(
            DomainEventKind::TaskCreated,
            Some(EntityId::from_bytes([3; 16])),
            root,
        );
        assert!(
            shell
                .complete(
                    pending,
                    Ok(Effect::new(
                        DomainEventKind::TaskCreated,
                        Some(EntityId::from_bytes([3; 16])),
                        root
                    ))
                )
                .is_err()
        );
        assert_eq!(
            (
                shell.receipts.len(),
                shell.events.len(),
                shell.nonce(verified.call.origin)
            ),
            before
        );
        effect.allocated_task_id = Some(7);
        let pending = prepared(&shell, verified.clone())?;
        let receipt = shell.complete(pending, Ok(effect))?;
        assert_eq!(
            shell
                .receipts
                .get(&receipt.operation_id)
                .ok_or_else(Error::unavailable)?
                .allocated_task_id,
            Some(7)
        );
        shell.validate(root)?;
        let reopened = shell.clone();
        assert!(matches!(
            reopened.prepare(verified.clone(), UnixSeconds::new(999))?,
            Admission::Replay(same) if same == receipt
        ));
        let mut conflicting = verified.clone();
        if let ProviderCommand::CreateTaskCore { assignees, .. } = &mut conflicting.call.command {
            assignees.push(root);
        }
        conflicting.bytes = conflicting.call.signable_bytes()?;
        assert_eq!(
            reopened
                .prepare(conflicting, UnixSeconds::new(999))
                .err()
                .map(|e| e.code()),
            Some("idempotency_conflict")
        );
        for invalid in [None, Some(0), Some(u32::MAX)] {
            shell
                .receipts
                .get_mut(&receipt.operation_id)
                .ok_or_else(Error::unavailable)?
                .allocated_task_id = invalid;
            assert_eq!(
                shell.validate(root).err().map(|e| e.code()),
                Some("invalid_task_allocation_state")
            );
        }
        let recorded = shell
            .receipts
            .get_mut(&receipt.operation_id)
            .ok_or_else(Error::unavailable)?;
        recorded.allocated_task_id = Some(7);
        recorded.receipt.outcome = ExecutionOutcome::Failed("task_storage_not_found".into());
        assert_eq!(
            shell.validate(root).err().map(|e| e.code()),
            Some("invalid_task_allocation_state")
        );
        shell
            .receipts
            .get_mut(&receipt.operation_id)
            .ok_or_else(Error::unavailable)?
            .allocated_task_id = None;
        shell.validate(root)?;
        Ok(())
    }
    #[test]
    fn task_allocation_read_binds_the_original_command_and_receipt() -> Result<()> {
        let root = AccountId32::from_bytes([1; 32]);
        let storage = EntityId::from_bytes([3; 16]);
        let project = EntityId::from_bytes([2; 16]);
        let mut shell = Shell::new(root)?;
        let mut verified = call(&shell, 1)?;
        verified.call.command = ProviderCommand::CreateTaskCore {
            project_id: project,
            task_storage_id: storage,
            assignees: Vec::new(),
        };
        verified.bytes = verified.call.signable_bytes()?;
        let pending = prepared(&shell, verified)?;
        let mut effect = Effect::new(DomainEventKind::TaskCreated, Some(storage), root);
        effect.allocated_task_id = Some(7);
        let receipt = shell.complete(pending, Ok(effect))?;
        let result = shell
            .task_allocation(receipt.operation_id)?
            .ok_or_else(Error::unavailable)?;
        assert_eq!(result.task_id, 7);
        assert_eq!(result.project_id, project);
        assert_eq!(result.task_storage_id, storage);
        assert_eq!(result.receipt, receipt);
        assert!(
            shell
                .task_allocation(OperationId::from_bytes([2; 16]))?
                .is_none()
        );
        shell
            .receipts
            .get_mut(&receipt.operation_id)
            .ok_or_else(Error::unavailable)?
            .receipt
            .created_entity_id = Some(project);
        assert!(
            shell
                .task_allocation(receipt.operation_id)
                .err()
                .ok_or_else(Error::unavailable)?
                .is_infrastructure()
        );
        Ok(())
    }
}
