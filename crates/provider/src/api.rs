//! Transport effect boundary shared by native and contract providers.
use crate::Result;
use protocol::provider_core::CoreDisputeView;
use protocol::provider_core::CoreSnapshot;
use protocol::provider_core::SkillRequestCore;
use protocol::{
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
    fn snapshot(&self) -> impl Future<Output = Result<CoreSnapshot>> + Send;
    /// Atomic task graph and retained next key from the selected authority.
    fn task_state(&self)
    -> impl Future<Output = Result<protocol::provider_core::TaskState>> + Send;
    /// Recorded finality independent of the original transport response.
    fn receipt(
        &self,
        operation: OperationId,
    ) -> impl Future<Output = Result<Option<OperationReceipt>>> + Send;
    /// Private core task key bound to its original command and successful receipt.
    fn task_allocation(
        &self,
        operation: OperationId,
    ) -> impl Future<Output = Result<Option<protocol::provider_core::TaskAllocation>>> + Send;
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
    ) -> impl Future<Output = Result<protocol::provider_core::ReasonPageV1>> + Send;
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
