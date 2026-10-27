//! Existing private HTTP provider contract over the bounded runtime worker.
use crate::worker::{Client, Error, Ticket};
use protocol::provider_core::CoreSnapshot;
use protocol::*;
use provider::{ProviderApi, Result};

fn transport(error: Error) -> provider::Error {
    match error {
        Error::Provider(error) => error,
        Error::Shell(error) => error.into(),
        _ => provider::Error::unavailable(),
    }
}
async fn result<T>(ticket: std::result::Result<Ticket<T>, Error>) -> Result<T> {
    ticket.map_err(transport)?.result().await.map_err(transport)
}
impl ProviderApi for Client {
    async fn info(&self) -> Result<ProviderInfo> {
        result(self.read_shell(|shell| Ok(shell.info))).await
    }
    async fn nonce(&self, account: AccountId32) -> Result<AccountNonce> {
        result(self.read_shell(move |shell| {
            Ok(AccountNonce {
                account,
                nonce: shell.nonce(account),
            })
        }))
        .await
    }
    async fn snapshot(&self) -> Result<CoreSnapshot> {
        result(self.read(|state| state.snapshot())).await
    }
    async fn task_state(&self) -> Result<protocol::provider_core::TaskState> {
        result(self.read(|state| state.task_state())).await
    }
    async fn receipt(&self, operation: OperationId) -> Result<Option<OperationReceipt>> {
        result(self.read_shell(move |shell| {
            Ok(shell
                .receipts
                .get(&operation)
                .map(|record| record.receipt.clone()))
        }))
        .await
    }
    async fn task_allocation(
        &self,
        operation: OperationId,
    ) -> Result<Option<protocol::provider_core::TaskAllocation>> {
        result(self.read(move |state| state.task_allocation(operation))).await
    }
    async fn events(&self, after: u64, limit: usize) -> Result<ProviderEvents> {
        if !(1..=1000).contains(&limit) {
            return Err(provider::Error::bad("invalid_event_limit"));
        }
        result(self.read_shell(move |shell| {
            let events: Vec<_> = shell
                .events
                .into_iter()
                .filter(|event| event.cursor > after)
                .take(limit)
                .collect();
            Ok(ProviderEvents {
                provider_instance_id: shell.info.provider_instance_id,
                next_cursor: events.last().map_or(after, |event| event.cursor),
                events,
            })
        }))
        .await
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
    ) -> Result<protocol::provider_core::ReasonPageV1> {
        result(self.read(move |state| state.lifecycle_reasons(after, limit))).await
    }
    async fn skill_requests(
        &self,
        origin: AccountId32,
    ) -> Result<Vec<protocol::provider_core::SkillRequestCore>> {
        result(self.read(move |state| state.skill_requests(origin))).await
    }
    async fn dispute(&self, id: EntityId) -> Result<protocol::provider_core::CoreDisputeView> {
        result(self.read(move |state| state.dispute(id))).await
    }
    async fn bramp_deposit(&self, origin: AccountId32, id: EntityId) -> Result<DepositView> {
        result(self.read(move |state| state.bramp_deposit(origin, id))).await
    }
    async fn bramp_withdrawal(&self, origin: AccountId32, id: EntityId) -> Result<WithdrawalView> {
        result(self.read(move |state| state.bramp_withdrawal(origin, id))).await
    }
}
