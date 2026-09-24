use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use generated_contracts::{
    AccountId32, AccountNonce, BalanceView, CatalogEntry, CatalogView, DomainEvent,
    DomainEventKind, EntityId, ExecutionOutcome, Minutes, Money, OperationId, OperationReceipt,
    PAYLOAD_VERSION, Percentage, ProjectView, ProviderCommand, ProviderEvents, ProviderInfo,
    ProviderInstanceId, ProviderSnapshot, ReputationView, ScorePolicy, SignedContractCallV1,
    UnixSeconds, UnsignedContractCallV1, WorkerView,
};
use serde::{Deserialize, Serialize};
use subxt_signer::sr25519;

use crate::{Error, Result, random_id, require, seed::SkillMetadata};

mod bramp;
mod commands;
mod dispute_validation;
mod disputes;
mod project;
mod submissions;
#[cfg(test)]
mod tests;
mod validation;

#[derive(Clone, Serialize, Deserialize)]
struct RecordedCall {
    bytes: Vec<u8>,
    receipt: OperationReceipt,
}

#[derive(Clone, Serialize, Deserialize)]
struct RecordedReason {
    project_id: EntityId,
    milestone_id: Option<EntityId>,
    origin: AccountId32,
    reason: String,
    occurred_at: UnixSeconds,
    kind: DomainEventKind,
}

/// Private aggregate: only authenticated calls can obtain a transition and only
/// storage can commit it. HTTP snapshots are copies, never mutable handles.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct State {
    info: ProviderInfo,
    roles: BTreeMap<u32, CatalogEntry>,
    skills: BTreeMap<u32, CatalogEntry>,
    skill_metadata: BTreeMap<u32, SkillMetadata>,
    score_policy: ScorePolicy,
    workers: BTreeMap<AccountId32, WorkerView>,
    projects: BTreeMap<EntityId, ProjectView>,
    balances: BTreeMap<AccountId32, Money>,
    nonces: BTreeMap<AccountId32, u64>,
    receipts: BTreeMap<OperationId, RecordedCall>,
    events: Vec<DomainEvent>,
    reasons: Vec<RecordedReason>,
    disputes: BTreeMap<EntityId, generated_contracts::Dispute>,
    #[serde(default)]
    bramp: bramp::BrampState,
    next_task_id: u32,
    minted_units: Money,
    #[serde(skip)]
    skill_index: BTreeMap<u32, BTreeSet<AccountId32>>,
}

/// Typestate gate: unverified envelopes cannot be handed to the state engine.
pub(crate) struct VerifiedCall {
    call: UnsignedContractCallV1,
    bytes: Vec<u8>,
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

struct Effect {
    kind: DomainEventKind,
    entity_id: Option<EntityId>,
    recipients: BTreeSet<AccountId32>,
}

impl Effect {
    fn new(kind: DomainEventKind, entity_id: Option<EntityId>, recipient: AccountId32) -> Self {
        Self {
            kind,
            entity_id,
            recipients: BTreeSet::from([recipient]),
        }
    }
}

impl State {
    pub(crate) fn new(root_account: AccountId32) -> Result<Self> {
        let mut state = Self {
            info: ProviderInfo {
                provider_instance_id: ProviderInstanceId::from_bytes(random_id()?.into_bytes()),
                root_account,
                payload_version: PAYLOAD_VERSION,
            },
            roles: BTreeMap::new(),
            skills: BTreeMap::new(),
            skill_metadata: BTreeMap::new(),
            score_policy: ScorePolicy::new(Percentage::new(50)?, Percentage::new(50)?)?,
            workers: BTreeMap::new(),
            projects: BTreeMap::new(),
            balances: BTreeMap::new(),
            nonces: BTreeMap::new(),
            receipts: BTreeMap::new(),
            events: Vec::new(),
            reasons: Vec::new(),
            disputes: BTreeMap::new(),
            bramp: bramp::BrampState::default(),
            next_task_id: 1,
            minted_units: Money::ZERO,
            skill_index: BTreeMap::new(),
        };
        state.roles.insert(
            1,
            CatalogEntry {
                id: 1,
                name: "coordinator".into(),
                fixed: true,
            },
        );
        state.seed();
        Ok(state)
    }

    pub(crate) fn seed(&mut self) {
        #[cfg(feature = "mock-seed")]
        crate::seed::seed(&mut self.roles, &mut self.skills, &mut self.skill_metadata);
    }

    pub(crate) fn info(&self) -> ProviderInfo {
        self.info.clone()
    }
    pub(crate) fn nonce(&self, account: AccountId32) -> AccountNonce {
        AccountNonce {
            account,
            nonce: self.nonces.get(&account).copied().unwrap_or(0),
        }
    }
    pub(crate) fn receipt(&self, operation: OperationId) -> Option<OperationReceipt> {
        self.receipts
            .get(&operation)
            .map(|record| record.receipt.clone())
    }
    pub(crate) fn snapshot(&self) -> ProviderSnapshot {
        ProviderSnapshot {
            info: self.info(),
            catalog: CatalogView {
                roles: self.roles.values().cloned().collect(),
                skills: self.skills.values().cloned().collect(),
                score_policy: self.score_policy,
            },
            workers: self.workers.values().cloned().collect(),
            projects: self.projects.values().cloned().collect(),
            balances: self
                .balances
                .iter()
                .map(|(account, available)| BalanceView {
                    account: *account,
                    asset_id: 1,
                    available: *available,
                })
                .collect(),
        }
    }
    pub(crate) fn events(&self, after: u64, limit: usize) -> ProviderEvents {
        let events: Vec<_> = self
            .events
            .iter()
            .filter(|event| event.cursor > after)
            .take(limit)
            .cloned()
            .collect();
        let next_cursor = events.last().map_or(after, |event| event.cursor);
        ProviderEvents {
            provider_instance_id: self.info.provider_instance_id,
            events,
            next_cursor,
        }
    }

    #[cfg(feature = "storage-sqlite")]
    pub(crate) fn restore(bytes: &[u8], root_account: AccountId32) -> Result<Self> {
        let mut state: Self = serde_json::from_slice(bytes)
            .map_err(|_| Error::domain("state_configuration_mismatch"))?;
        require(
            state.info.root_account == root_account
                && state.info.payload_version == PAYLOAD_VERSION,
            "state_configuration_mismatch",
        )?;
        state.validate()?;
        state.rebuild_index();
        Ok(state)
    }
    #[cfg(feature = "storage-sqlite")]
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|_| Error::internal())
    }
    fn rebuild_index(&mut self) {
        self.skill_index.clear();
        for worker in self.workers.values() {
            for skill in &worker.qualifications.skill_ids {
                self.skill_index
                    .entry(*skill)
                    .or_default()
                    .insert(worker.account);
            }
        }
    }

    /// Failed domain execution commits only its nonce and failed receipt; its
    /// tentative business state and events are discarded as a unit.
    pub(crate) fn execute(
        mut self,
        verified: VerifiedCall,
        now: UnixSeconds,
    ) -> Result<(Self, OperationReceipt)> {
        let VerifiedCall { call, bytes } = verified;
        require(
            call.provider_instance_id == self.info.provider_instance_id,
            "provider_instance_mismatch",
        )?;
        if let Some(record) = self.receipts.get(&call.operation_id) {
            require(record.bytes == bytes, "idempotency_conflict")?;
            let receipt = record.receipt.clone();
            return Ok((self, receipt));
        }
        require(call.expires_at > now, "expired_call")?;
        require(
            call.nonce == self.nonce(call.origin).nonce,
            "nonce_conflict",
        )?;
        let next_nonce = call
            .nonce
            .checked_add(1)
            .ok_or_else(|| Error::domain("nonce_exhausted"))?;
        let mut tentative = self.clone();
        let transition = tentative.apply(call.origin, &call.command, now);
        let (outcome, entity_id, cursor) = match transition {
            Ok(effect) => {
                tentative.validate()?;
                let cursor = u64::try_from(tentative.events.len())
                    .map_err(|_| Error::internal())?
                    .checked_add(1)
                    .ok_or_else(Error::internal)?;
                tentative.events.push(DomainEvent {
                    provider_instance_id: self.info.provider_instance_id,
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
                });
                self = tentative;
                (ExecutionOutcome::Success, effect.entity_id, Some(cursor))
            }
            Err(error) if error.status == axum::http::StatusCode::SERVICE_UNAVAILABLE => {
                return Err(error);
            }
            Err(error) => (ExecutionOutcome::Failed(error.code().into()), None, None),
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
            finalized_at: now,
        };
        self.nonces.insert(call.origin, next_nonce);
        self.receipts.insert(
            call.operation_id,
            RecordedCall {
                bytes,
                receipt: receipt.clone(),
            },
        );
        Ok((self, receipt))
    }
}

fn empty_score() -> ReputationView {
    ReputationView {
        weighted_score_sum: 0,
        rated_minutes: 0,
    }
}
fn validate_score(score: &ReputationView) -> Result<()> {
    require(
        score.weighted_score_sum <= u128::from(score.rated_minutes) * 1000,
        "invalid_score_state",
    )
}
fn add_rating(score: &mut ReputationView, hundredths: u16, minutes: Minutes) -> Result<()> {
    score.rated_minutes = score
        .rated_minutes
        .checked_add(u64::from(minutes.get()))
        .ok_or_else(|| Error::domain("score_overflow"))?;
    score.weighted_score_sum = score
        .weighted_score_sum
        .checked_add(u128::from(hundredths) * u128::from(minutes.get()))
        .ok_or_else(|| Error::domain("score_overflow"))?;
    validate_score(score)
}
fn score_fraction(score: &ReputationView) -> (u128, u128) {
    if score.rated_minutes == 0 {
        (500, 1)
    } else {
        (score.weighted_score_sum, u128::from(score.rated_minutes))
    }
}
/// Continued fractions compare exact ratios without overflowing cross-products.
fn compare_scores(left: &ReputationView, right: &ReputationView) -> Ordering {
    let (mut ln, mut ld) = score_fraction(left);
    let (mut rn, mut rd) = score_fraction(right);
    let mut inverted = false;
    loop {
        let mut order = (ln / ld).cmp(&(rn / rd));
        if order != Ordering::Equal {
            return if inverted { order.reverse() } else { order };
        }
        let (lr, rr) = (ln % ld, rn % rd);
        if lr == 0 || rr == 0 {
            order = lr.cmp(&rr);
            return if inverted { order.reverse() } else { order };
        }
        (ln, ld, rn, rd) = (ld, lr, rd, rr);
        inverted = !inverted;
    }
}
