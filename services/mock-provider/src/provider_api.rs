//! Transport effect boundary shared by native and whole-provider candidates.
use crate::{Provider, Result};
use generated_contracts::{
    AccountId32, AccountNonce, DepositView, DisputeView, EntityId, OperationId, OperationReceipt,
    ProviderEvents, ProviderInfo, ProviderSnapshot, SignedContractCallV1, SkillRequestView,
    UnixSeconds, WithdrawalView,
};
use std::future::Future;

/// The private HTTP contract; implementations must preserve the signed shell.
pub trait ProviderApi: Clone + Send + Sync + 'static {
    /// Generation and root identity used for signing.
    fn info(&self) -> impl Future<Output = Result<ProviderInfo>> + Send;
    /// Next nonce for a new account operation.
    fn nonce(&self, account: AccountId32) -> impl Future<Output = Result<AccountNonce>> + Send;
    /// Internal read model; the adapter still filters project visibility.
    fn snapshot(&self) -> impl Future<Output = Result<ProviderSnapshot>> + Send;
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
    /// Apply one verified call with captured inputs; no storage commit occurs here.
    fn execute(
        &self,
        call: SignedContractCallV1,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<OperationReceipt>> + Send;
    /// Requests visible to their worker or the configured system account.
    fn skill_requests(
        &self,
        origin: AccountId32,
    ) -> impl Future<Output = Result<Vec<SkillRequestView>>> + Send;
    /// Public case projection without internal provider state.
    fn dispute(&self, id: EntityId) -> impl Future<Output = Result<DisputeView>> + Send;
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
    async fn snapshot(&self) -> Result<ProviderSnapshot> {
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
    async fn skill_requests(&self, origin: AccountId32) -> Result<Vec<SkillRequestView>> {
        Self::skill_requests(self, origin).await
    }
    async fn dispute(&self, id: EntityId) -> Result<DisputeView> {
        Self::dispute(self, id).await
    }
    async fn bramp_deposit(&self, origin: AccountId32, id: EntityId) -> Result<DepositView> {
        Self::bramp_deposit(self, origin, id).await
    }
    async fn bramp_withdrawal(&self, origin: AccountId32, id: EntityId) -> Result<WithdrawalView> {
        Self::bramp_withdrawal(self, origin, id).await
    }
}
