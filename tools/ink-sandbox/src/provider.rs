//! Existing private HTTP provider contract over the bounded runtime worker.
use crate::worker::{Client, Error, Ticket};
use generated_contracts::provider_core::CoreSnapshotV5;
use generated_contracts::*;
use mock_provider::{ProviderApi, ReferenceState, Result};

fn transport(error: Error) -> mock_provider::Error {
    match error {
        Error::Reference(error) => error,
        Error::Shell(error) => error.into(),
        _ => mock_provider::Error::unavailable(),
    }
}
async fn result<T>(ticket: std::result::Result<Ticket<T>, Error>) -> Result<T> {
    ticket.map_err(transport)?.result().await.map_err(transport)
}
impl Client {
    async fn read<T: Send + 'static>(
        &self,
        read: impl FnOnce(ReferenceState) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        result(self.read_reference(read)).await
    }
}
impl ProviderApi for Client {
    async fn info(&self) -> Result<ProviderInfo> {
        self.read(|state| Ok(state.info())).await
    }
    async fn nonce(&self, account: AccountId32) -> Result<AccountNonce> {
        self.read(move |state| Ok(state.nonce(account))).await
    }
    async fn snapshot(&self) -> Result<CoreSnapshotV5> {
        self.read(|state| Ok(state.snapshot())).await
    }
    async fn task_state(&self) -> Result<generated_contracts::provider_core::TaskStateV2> {
        self.read(|state| Ok(state.task_state())).await
    }
    async fn receipt(&self, operation: OperationId) -> Result<Option<OperationReceipt>> {
        self.read(move |state| Ok(state.receipt(operation))).await
    }
    async fn task_allocation(
        &self,
        operation: OperationId,
    ) -> Result<Option<generated_contracts::provider_core::TaskAllocationV1>> {
        self.read(move |state| state.task_allocation(operation))
            .await
    }
    async fn events(&self, after: u64, limit: usize) -> Result<ProviderEvents> {
        self.read(move |state| state.events(after, limit)).await
    }
    async fn execute(
        &self,
        call: SignedContractCallV1,
        now: UnixSeconds,
    ) -> Result<OperationReceipt> {
        // This pre-queue limit is an invalid envelope, as in native admission.
        let ticket = match self.execute_signed(call, now) {
            Err(Error::Limit) => return Err(ContractError::InvalidPayload.into()),
            ticket => ticket,
        };
        result(ticket).await
    }
    async fn lifecycle_reasons(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<generated_contracts::provider_core::ReasonPageV1> {
        self.read(move |state| state.lifecycle_reasons(after, limit))
            .await
    }
    async fn skill_requests(
        &self,
        origin: AccountId32,
    ) -> Result<Vec<generated_contracts::provider_core::SkillRequestCore>> {
        self.read(move |state| Ok(state.skill_requests(origin)))
            .await
    }
    async fn dispute(
        &self,
        id: EntityId,
    ) -> Result<generated_contracts::provider_core::CoreDisputeView> {
        self.read(move |state| state.dispute(id)).await
    }
    async fn bramp_deposit(&self, origin: AccountId32, id: EntityId) -> Result<DepositView> {
        self.read(move |state| state.bramp_deposit(origin, id))
            .await
    }
    async fn bramp_withdrawal(&self, origin: AccountId32, id: EntityId) -> Result<WithdrawalView> {
        self.read(move |state| state.bramp_withdrawal(origin, id))
            .await
    }
}
