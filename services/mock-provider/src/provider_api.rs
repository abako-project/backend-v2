//! Transport effect boundary shared by native and whole-provider candidates.
use crate::{Provider, Result};
use generated_contracts::provider_core::CoreDisputeView;
use generated_contracts::provider_core::CoreSnapshotV4;
use generated_contracts::provider_core::SkillRequestCore;
use generated_contracts::{
    AccountId32, AccountNonce, DepositView, EntityId, OperationId, OperationReceipt,
    ProviderEvents, ProviderInfo, SignedContractCallV1, UnixSeconds, WithdrawalView,
};
use std::future::Future;

/// The private HTTP contract; implementations must preserve the signed shell.
pub trait ProviderApi: Clone + Send + Sync + 'static {
    /// Generation and root identity used for signing.
    fn info(&self) -> impl Future<Output = Result<ProviderInfo>> + Send;
    /// Next nonce for a new account operation.
    fn nonce(&self, account: AccountId32) -> impl Future<Output = Result<AccountNonce>> + Send;
    /// Internal read model; the adapter still filters project visibility.
    fn snapshot(&self) -> impl Future<Output = Result<CoreSnapshotV4>> + Send;
    /// Recorded finality independent of the original transport response.
    fn receipt(
        &self,
        operation: OperationId,
    ) -> impl Future<Output = Result<Option<OperationReceipt>>> + Send;
    /// Bounded page of durable provider events.
    fn events(
        &self,
        after: u64,
        limit: usize,
    ) -> impl Future<Output = Result<ProviderEvents>> + Send;
    /// Execute a signed call through the provider's atomic command boundary.
    fn execute(
        &self,
        call: SignedContractCallV1,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<OperationReceipt>> + Send;
    /// Bounded private lifecycle facts; no explanatory text is returned.
    fn lifecycle_reasons(
        &self,
        after: u64,
        limit: usize,
    ) -> impl Future<Output = Result<generated_contracts::provider_core::ReasonPageV1>> + Send;
    /// Requests visible to their worker or the configured system account.
    fn skill_requests(
        &self,
        origin: AccountId32,
    ) -> impl Future<Output = Result<Vec<SkillRequestCore>>> + Send;
    /// Public case projection without internal provider state.
    fn dispute(&self, id: EntityId) -> impl Future<Output = Result<CoreDisputeView>> + Send;
    /// Read a deposit using the existing owner/system policy.
    fn bramp_deposit(
        &self,
        origin: AccountId32,
        id: EntityId,
    ) -> impl Future<Output = Result<DepositView>> + Send;
    /// Read a withdrawal using the existing owner/system policy.
    fn bramp_withdrawal(
        &self,
        origin: AccountId32,
        id: EntityId,
    ) -> impl Future<Output = Result<WithdrawalView>> + Send;
}
impl ProviderApi for Provider {
    async fn info(&self) -> Result<ProviderInfo> {
        Self::info(self).await
    }
    async fn nonce(&self, account: AccountId32) -> Result<AccountNonce> {
        Self::nonce(self, account).await
    }
    async fn snapshot(&self) -> Result<CoreSnapshotV4> {
        Self::snapshot(self).await
    }
    async fn receipt(&self, operation: OperationId) -> Result<Option<OperationReceipt>> {
        Self::receipt(self, operation).await
    }
    async fn events(&self, after: u64, limit: usize) -> Result<ProviderEvents> {
        Self::events(self, after, limit).await
    }
    async fn execute(
        &self,
        call: SignedContractCallV1,
        now: UnixSeconds,
    ) -> Result<OperationReceipt> {
        Self::execute(self, call, now).await
    }
    async fn lifecycle_reasons(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<generated_contracts::provider_core::ReasonPageV1> {
        Self::lifecycle_reasons(self, after, limit).await
    }
    async fn skill_requests(&self, origin: AccountId32) -> Result<Vec<SkillRequestCore>> {
        Self::skill_requests(self, origin).await
    }
    async fn dispute(&self, id: EntityId) -> Result<CoreDisputeView> {
        Self::dispute(self, id).await
    }
    async fn bramp_deposit(&self, origin: AccountId32, id: EntityId) -> Result<DepositView> {
        Self::bramp_deposit(self, origin, id).await
    }
    async fn bramp_withdrawal(&self, origin: AccountId32, id: EntityId) -> Result<WithdrawalView> {
        Self::bramp_withdrawal(self, origin, id).await
    }
}
