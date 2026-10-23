//! Temporary native reference and read models for the whole-provider rollout.
use crate::{Error, Result, domain::State};
use generated_contracts::provider_core::CoreDisputeView;
use generated_contracts::provider_core::CoreSnapshotV4;
use generated_contracts::provider_core::SkillRequestCore;
use generated_contracts::{
    AccountId32, AccountNonce, DepositView, EntityId, OperationId, OperationReceipt,
    ProviderEvents, ProviderInfo, WithdrawalView,
};
use provider_execution::{ExecutionInputs, VerifiedCall, migration::CoreArchiveV4};

/// Private-engine bridge; construction validates a complete retained archive.
/// These synchronous methods belong on a dedicated worker or blocking pool.
/// Only a cryptographically verified envelope can enter execution.
pub struct ReferenceState(State);
impl ReferenceState {
    /// Validate domain and retained shell before creating the reference.
    pub fn from_archive(root: AccountId32, archive: CoreArchiveV4) -> Result<Self> {
        State::from_archive(archive, root).map(Self)
    }
    /// Apply one verified call with captured inputs; no storage commit occurs here.
    pub fn execute(
        self,
        call: VerifiedCall,
        inputs: ExecutionInputs,
    ) -> Result<(Self, OperationReceipt)> {
        self.0
            .execute_with_inputs(call, inputs)
            .map(|(state, receipt)| (Self(state), receipt))
    }
    /// Export complete current domain and shell state.
    pub fn archive(&self) -> Result<CoreArchiveV4> {
        self.0.archive()
    }
    /// Generation and root identity used for signing.
    #[must_use]
    pub fn info(&self) -> ProviderInfo {
        self.0.info()
    }
    /// Next nonce for a new account operation.
    #[must_use]
    pub fn nonce(&self, account: AccountId32) -> AccountNonce {
        self.0.nonce(account)
    }
    /// Internal read model; the adapter still filters project visibility.
    #[must_use]
    pub fn snapshot(&self) -> CoreSnapshotV4 {
        self.0.snapshot()
    }
    /// Recorded finality independent of the original transport response.
    #[must_use]
    pub fn receipt(&self, operation: OperationId) -> Option<OperationReceipt> {
        self.0.receipt(operation)
    }
    /// Bounded page of durable provider events.
    pub fn events(&self, after: u64, limit: usize) -> Result<ProviderEvents> {
        if limit == 0 || limit > 1000 {
            return Err(Error::bad("invalid_event_limit"));
        }
        Ok(self.0.events(after, limit))
    }
    /// Bounded private lifecycle facts for explicit owner metadata exchange.
    pub fn lifecycle_reasons(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<generated_contracts::provider_core::ReasonPageV1> {
        self.0.lifecycle_reasons(after, limit)
    }
    /// Requests visible to their worker or the configured system account.
    #[must_use]
    pub fn skill_requests(&self, origin: AccountId32) -> Vec<SkillRequestCore> {
        self.0.skill_requests(origin)
    }
    /// Public case projection without internal provider state.
    pub fn dispute(&self, id: EntityId) -> Result<CoreDisputeView> {
        self.0.dispute_view(id)
    }
    /// Read a deposit using the existing owner/system policy.
    pub fn bramp_deposit(&self, origin: AccountId32, id: EntityId) -> Result<DepositView> {
        self.0.bramp_deposit(origin, id)
    }
    /// Read a withdrawal using the existing owner/system policy.
    pub fn bramp_withdrawal(&self, origin: AccountId32, id: EntityId) -> Result<WithdrawalView> {
        self.0.bramp_withdrawal(origin, id)
    }
}
