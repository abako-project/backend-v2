//! Rust engine integration with the shared signed-operation shell.
use super::State;
use crate::{Error, Result};
use generated_contracts::{AccountId32, OperationReceipt, UnixSeconds};
pub(crate) use provider_execution::VerifiedCall;
use provider_execution::{Admission, PreparedExecution};
pub(super) use provider_execution::{ExecutionInputs, Shell};
use rand::{Rng, RngExt, SeedableRng, rngs::StdRng};
use std::cell::RefCell;

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
impl State {
    pub(crate) fn execute(
        self,
        verified: VerifiedCall,
        now: UnixSeconds,
    ) -> Result<(Self, OperationReceipt)> {
        match self.shell.prepare(verified, now)? {
            Admission::Replay(receipt) => Ok((self, receipt)),
            Admission::Execute(pending) => self.execute_prepared(*pending),
        }
    }
    #[cfg(all(test, feature = "mock-seed"))]
    pub(crate) fn execute_with_inputs(
        self,
        verified: VerifiedCall,
        inputs: ExecutionInputs,
    ) -> Result<(Self, OperationReceipt)> {
        match self.shell.prepare_with_inputs(verified, inputs)? {
            Admission::Replay(receipt) => Ok((self, receipt)),
            Admission::Execute(pending) => self.execute_prepared(*pending),
        }
    }
    fn execute_prepared(mut self, pending: PreparedExecution) -> Result<(Self, OperationReceipt)> {
        let mut tentative = self.engine.clone();
        tentative.context.reset(pending.inputs());
        let call = pending.call();
        let compact = crate::creation::compact_creation(call)
            .or_else(|| crate::proposal::compact_proposal(call))
            .or_else(|| crate::catalog_content::compact_catalog(call))
            .or_else(|| crate::catalog_content::compact_reason(call));
        let command = compact.as_ref().unwrap_or(&call.command);
        let transition = tentative.apply_core(call.origin, command, pending.inputs().now);
        let transition = match transition {
            Ok(effect) => {
                tentative.validate()?;
                Ok(effect)
            }
            Err(error) if error.status == axum::http::StatusCode::SERVICE_UNAVAILABLE => {
                return Err(error);
            }
            Err(error) => Err(error.code()),
        };
        let success = transition.is_ok();
        let receipt = self.shell.complete(pending, transition)?;
        if success {
            tentative.context.event_cursor =
                receipt.last_event_cursor.ok_or_else(Error::internal)?;
            self.engine = tentative;
        }
        Ok((self, receipt))
    }
    #[cfg(feature = "storage-sqlite")]
    pub(super) fn validate_history(&self) -> Result<()> {
        self.shell.validate(self.shell.info.root_account)?;
        for worker in self.engine.workers.values() {
            self.validate_creation_ref(
                &worker.content,
                worker.account,
                worker.calendar.calendar_id,
                true,
            )?;
        }
        for project in self.engine.projects.values() {
            self.validate_creation_ref(
                &project.content,
                project.client,
                project.project_id,
                false,
            )?;
            for proposal in &project.proposals {
                self.validate_proposal_ref(project, proposal, &proposal.content, false)?;
                if let Some(reference) = &proposal.change_request {
                    self.validate_proposal_ref(project, proposal, reference, true)?;
                }
            }
        }
        self.validate_catalog_history()?;
        self.validate_reason_history()?;
        Ok(())
    }
    #[cfg(feature = "storage-sqlite")]
    fn validate_proposal_ref(
        &self,
        project: &generated_contracts::provider_core::ProjectCore,
        proposal: &generated_contracts::provider_core::ProposalCore,
        reference: &generated_contracts::provider_core::ProposalContentReferenceV1,
        change: bool,
    ) -> Result<()> {
        use generated_contracts::{
            ExecutionOutcome, ProviderCommand, UnsignedContractCallV1,
            provider_core::ProposalContentReferenceV1,
        };
        let ProposalContentReferenceV1::Signed(expected) = reference;
        let accepted = self
            .shell
            .receipts
            .get(&expected.operation_id)
            .ok_or_else(|| Error::domain("invalid_proposal_metadata_state"))?;
        crate::require(
            accepted.receipt.outcome == ExecutionOutcome::Success
                && accepted.receipt.origin
                    == if change {
                        project.client
                    } else {
                        project.coordinator
                    }
                && accepted.receipt.created_entity_id == Some(proposal.proposal_id),
            "invalid_proposal_metadata_state",
        )?;
        let call = UnsignedContractCallV1::decode_signable(&accepted.bytes)?;
        crate::require(
            call.command.project_id() == Some(project.project_id),
            "invalid_proposal_metadata_state",
        )?;
        let lowered = crate::proposal::compact_proposal(&call);
        match lowered.as_ref().unwrap_or(&call.command) {
            ProviderCommand::CreateProposalCore {
                content,
                proposal: terms,
                ..
            } if !change => Self::validate_quoted_proposal(proposal, expected, content, terms),
            ProviderCommand::UpdateProposalCore {
                proposal_id,
                content,
                proposal: terms,
                ..
            } if !change && *proposal_id == proposal.proposal_id => {
                Self::validate_quoted_proposal(proposal, expected, content, terms)
            }
            ProviderCommand::RequestProposalChangesCore {
                proposal_id,
                content,
                ..
            } if change && *proposal_id == proposal.proposal_id => {
                crate::require(content == expected, "invalid_proposal_metadata_state")
            }
            _ => Err(Error::domain("invalid_proposal_metadata_state")),
        }
    }
    #[cfg(feature = "storage-sqlite")]
    fn validate_quoted_proposal(
        proposal: &generated_contracts::provider_core::ProposalCore,
        expected: &generated_contracts::provider_core::MetadataCommitmentV1,
        actual: &generated_contracts::provider_core::MetadataCommitmentV1,
        terms: &generated_contracts::provider_core::ProposalTerms,
    ) -> Result<()> {
        let mut allocated = terms.clone();
        let policy = proposal
            .compensation_policy
            .as_ref()
            .ok_or_else(|| Error::domain("invalid_proposal_metadata_state"))?;
        for milestone in &mut allocated.milestones {
            milestone.allocate_budget(policy)?;
        }
        crate::require(
            actual == expected && allocated == proposal.terms(),
            "invalid_proposal_metadata_state",
        )
    }
    #[cfg(feature = "storage-sqlite")]
    fn validate_creation_ref(
        &self,
        reference: &generated_contracts::provider_core::MetadataReferenceV1,
        owner: AccountId32,
        entity: generated_contracts::EntityId,
        worker: bool,
    ) -> Result<()> {
        use generated_contracts::{ExecutionOutcome, ProviderCommand, UnsignedContractCallV1};
        let generated_contracts::provider_core::MetadataReferenceV1::Creation(expected) = reference;
        let recorded = self
            .shell
            .receipts
            .get(&expected.operation_id)
            .ok_or_else(|| Error::domain("invalid_metadata_state"))?;
        crate::require(
            recorded.receipt.outcome == ExecutionOutcome::Success
                && recorded.receipt.origin == owner
                && recorded.receipt.created_entity_id == Some(entity),
            "invalid_metadata_state",
        )?;
        let call = UnsignedContractCallV1::decode_signable(&recorded.bytes)?;
        let compact = crate::creation::compact_creation(&call);
        let actual = match compact.as_ref().unwrap_or(&call.command) {
            ProviderCommand::RegisterWorkerCore(request) if worker => &request.content,
            ProviderCommand::CreateProjectCore(content) if !worker => content,
            _ => return Err(Error::domain("invalid_metadata_state")),
        };
        crate::require(actual == expected, "invalid_metadata_state")
    }
}

#[cfg(all(test, feature = "mock-seed"))]
#[path = "directory_parity.rs"]
mod directory_parity;

#[cfg(all(test, feature = "mock-seed"))]
#[path = "matching_parity.rs"]
mod matching_parity;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::require;
    use generated_contracts::{OperationId, ProviderCommand, UnsignedContractCallV1};
    use subxt_signer::sr25519;
    fn key() -> Result<sr25519::Keypair> {
        sr25519::Keypair::from_secret_key([1; 32]).map_err(|_| Error::internal())
    }
    fn root() -> Result<AccountId32> {
        Ok(AccountId32::from_bytes(key()?.public_key().0))
    }
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
        let key = key()?;
        require(
            origin == AccountId32::from_bytes(key.public_key().0),
            "invalid_test_origin",
        )?;
        let signature =
            generated_contracts::Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0);
        Ok(VerifiedCall::verify(
            generated_contracts::SignedContractCallV1 { call, signature },
        )?)
    }

    #[test]
    fn identical_inputs_reproduce_generated_ids_and_exact_tie_selection() -> Result<()> {
        let root = root()?;
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
            .seed;
        let (replayed, receipt) =
            first.execute(call(&before, root, command)?, UnixSeconds::new(999))?;
        assert_eq!(receipt, first_receipt);
        assert_eq!(
            replayed.shell.receipts[&receipt.operation_id].inputs.seed,
            retained_seed
        );
        assert_eq!(replayed.shell.events.len(), 1);
        Ok(())
    }

    #[cfg(feature = "storage-sqlite")]
    #[test]
    fn checkpoint_requires_complete_execution_history() -> Result<()> {
        let root = root()?;
        let initial = State::new(root)?;
        let verified = call(&initial, root, worker_command())?;
        let replay = verified.clone();
        let (state, receipt) = initial.execute(verified, UnixSeconds::new(100))?;
        let saved = serde_json::to_value(&state).map_err(|_| Error::internal())?;
        let restored = State::restore(&state.encode()?, root)?;
        assert_eq!(restored.info(), state.info());
        assert_eq!(restored.nonce(root), state.nonce(root));
        assert_eq!(restored.snapshot(), state.snapshot());
        assert_eq!(restored.events(0, 100), state.events(0, 100));
        let (replayed, same) = restored.execute(replay, UnixSeconds::new(999))?;
        assert_eq!(same, receipt);
        assert_eq!(replayed.shell.events.len(), 1);

        let mut missing_inputs = saved.clone();
        for recorded in missing_inputs["receipts"]
            .as_object_mut()
            .ok_or_else(Error::internal)?
            .values_mut()
        {
            recorded
                .as_object_mut()
                .ok_or_else(Error::internal)?
                .remove("inputs");
        }
        let mut missing_version = saved.clone();
        missing_version
            .as_object_mut()
            .ok_or_else(Error::internal)?
            .remove("engine_version");
        let mut wrong_version = saved;
        wrong_version["engine_version"] = serde_json::json!(99);
        for incomplete in [missing_inputs, missing_version, wrong_version] {
            assert!(
                State::restore(
                    &serde_json::to_vec(&incomplete).map_err(|_| Error::internal())?,
                    root,
                )
                .is_err()
            );
        }
        Ok(())
    }
    #[cfg(feature = "storage-sqlite")]
    #[test]
    fn checkpoint_keeps_scores_larger_than_a_json_u64_exact() -> Result<()> {
        let root = root()?;
        let initial = State::new(root)?;
        let verified = call(&initial, root, worker_command())?;
        let (mut state, _) = initial.execute(verified, UnixSeconds::new(100))?;
        let score = generated_contracts::ReputationView {
            weighted_score_sum: u128::from(u64::MAX) * 499,
            rated_minutes: u64::MAX,
        };
        state.engine.worker_mut(root)?.worker_score = score.clone();
        let restored = State::restore(&state.encode()?, root)?;
        assert_eq!(restored.engine.workers[&root].worker_score, score);
        Ok(())
    }
    #[test]
    fn task_allocation_history_roundtrip_preserves_optional_private_id()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let before = State::new(root()?)?;
        let verified = call(&before, root()?, worker_command())?;
        let (after, receipt) = before.execute_with_inputs(
            verified,
            ExecutionInputs {
                version: 1,
                now: UnixSeconds::new(100),
                seed: [1; 32],
            },
        )?;
        let record = after
            .shell
            .receipts
            .get(&receipt.operation_id)
            .ok_or("missing receipt")?;
        let saved = serde_json::to_value(record)?;
        assert!(saved.get("allocated_task_id").is_none());
        let roundtrip: provider_execution::RecordedCall = serde_json::from_value(saved.clone())?;
        assert_eq!(serde_json::to_value(&roundtrip)?, saved);
        // Non-task operations have no allocation. Task creation retains its private ID.
        let mut allocated = roundtrip;
        allocated.allocated_task_id = Some(7);
        let encoded = serde_json::to_value(&allocated)?;
        let recovered: provider_execution::RecordedCall = serde_json::from_value(encoded.clone())?;
        assert_eq!(recovered.allocated_task_id, Some(7));
        assert_eq!(serde_json::to_value(&recovered)?, encoded);
        Ok(())
    }
}
