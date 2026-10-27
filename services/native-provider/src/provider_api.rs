use crate::{Provider, ProviderApi, Result};
use protocol::provider_core::CoreDisputeView;
use protocol::provider_core::CoreSnapshot;
use protocol::provider_core::SkillRequestCore;
use protocol::{
    AccountId32, AccountNonce, DepositView, EntityId, OperationId, OperationReceipt,
    ProviderEvents, ProviderInfo, SignedContractCallV1, UnixSeconds, WithdrawalView,
};

impl ProviderApi for Provider {
    async fn info(&self) -> Result<ProviderInfo> {
        Self::info(self).await
    }
    async fn nonce(&self, account: AccountId32) -> Result<AccountNonce> {
        Self::nonce(self, account).await
    }
    async fn snapshot(&self) -> Result<CoreSnapshot> {
        Self::snapshot(self).await
    }
    async fn task_state(&self) -> Result<protocol::provider_core::TaskState> {
        Self::task_state(self).await
    }
    async fn receipt(&self, operation: OperationId) -> Result<Option<OperationReceipt>> {
        Self::receipt(self, operation).await
    }
    async fn task_allocation(
        &self,
        operation: OperationId,
    ) -> Result<Option<protocol::provider_core::TaskAllocation>> {
        Self::task_allocation(self, operation).await
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
    ) -> Result<protocol::provider_core::ReasonPageV1> {
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
