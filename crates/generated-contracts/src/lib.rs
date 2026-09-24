//! Shared REST DTOs and versioned mock signing contracts, not database entities.
//!
//! JSON fields use camelCase; enums use their declared `PascalCase` names. All
//! amounts are integer KVN units encoded as decimal strings. Opaque IDs and
//! signatures are 0x-prefixed hex. See `API.md` for HTTP route mappings.
//!
//! Every boundary accepting a command must call [`ProviderCommand::validate`].
//! Signed bytes must be parsed with [`UnsignedContractCallV1::decode_signable`],
//! which rejects trailing bytes, unsupported domains and malformed commands.
#![allow(clippy::missing_errors_doc)]

pub use domain_primitives::*;
mod disputes;
pub use disputes::*;
mod bramp;
pub use bramp::*;
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// Signing domain prevents interpreting this format as another protocol.
pub const MOCK_SIGNING_DOMAIN: [u8; 16] = *b"KUNVENO-MOCK-V1!";
/// The signed SCALE format, independent of the public HTTP path.
pub const PAYLOAD_VERSION: u16 = 2;
/// Upper bound on signable transport bytes, not on matching candidates.
pub const MAX_SIGNABLE_BYTES: usize = 256 * 1024;

/// Structural wire-contract validation failure; never carries secret content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractError {
    /// Invalid nested primitive.
    #[error(transparent)]
    Primitive(#[from] ValidationError),
    /// Invalid field or cross-field relationship.
    #[error("invalid {0}")]
    Invalid(&'static str),
    /// Unsupported signing domain, provider format, or malformed SCALE bytes.
    #[error("invalid or unsupported signable payload")]
    InvalidPayload,
}

macro_rules! dto {
    ($(#[$attr:meta])* $name:ident { $($(#[$field_attr:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$attr])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name { $(
            #[doc = concat!("Wire field `", stringify!($field), "`.")]
            $(#[$field_attr])*
            pub $field: $ty,
        )* }
    };
}
macro_rules! wire_enum {
    ($(#[$attr:meta])* $name:ident { $($variant:ident),* $(,)? }) => {
        $(#[$attr])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
        pub enum $name { $( #[doc = stringify!($variant)] $variant, )* }
    };
}
// Credentials and session tokens must remain redacted even in accidental debug logs.
macro_rules! redacted_dto {
    ($(#[$attr:meta])* $name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        $(#[$attr])*
        #[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name { $(
            #[doc = concat!("Wire field `", stringify!($field), "`.")]
            pub $field: $ty,
        )* }
        impl std::fmt::Debug for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(concat!(stringify!($name), " { [REDACTED] }"))
            }
        }
    };
}

wire_enum!(/// Only one mode can be active; coordinator eligibility is separate.
    WorkerMode { Worker, Coordinator });
wire_enum!(/// Proposal lifecycle, distinct from execution lifecycle.
    ProposalStatus { Draft, PendingApproval, Approved, Cancelled });
wire_enum!(/// Absent until execution approval; funded milestones activate in order.
    MilestoneStatus { NotStarted, InProgress, CompletionRequested, Completed, Disputed, ChangesRequested });
wire_enum!(/// Planning delivery acceptance is separate from execution approval.
    PlanningStatus { AwaitingQuote, Quoted, Accepted, Delivered, Completed, Disputed });
wire_enum!(/// Tracking vocabulary retained from the legacy provider.
    TaskType { Feature, Bug, Task, Epic, Story });
wire_enum!(/// Six tracking priorities, ordered only by business UI convention.
    TaskPriority { Lowest, Low, Medium, High, Highest, Blocker });
wire_enum!(/// Tracking status; it does not settle payments or reservations.
    TaskStatus { ToDo, Open, InProgress, InReview, Done, Closed });
wire_enum!(/// Catalog ownership is provider-side.
    CatalogKind { Role, Skill });
wire_enum!(/// Key lifecycle; only Active may sign.
    WalletLifecycle { Provisioning, Active, Suspended, Retired });
wire_enum!(/// Durable custody job stage.
    SigningJobStatus { Pending, Signed, Rejected });
wire_enum!(/// Unknown submission outcome is explicitly not a rejection.
    ProviderOperationStatus { AwaitingSignature, ReadyToSubmit, Submitted, OutcomeUnknown, Finalized, Rejected, Expired });

dto!(/// Qualification metadata; team matching requires every skill, not the role.
    Qualifications { role_ids: Vec<u32>, skill_ids: Vec<u32> });
dto!(/// One explicit weekly capacity replacing the default for that week.
    WeekOverride { week: Week, capacity: Minutes });
dto!(/// Complete calendar capacity definition; reservations are never overwritten.
    CalendarDefinition { default_weekly_minutes: Minutes, overrides: Vec<WeekOverride> });
dto!(/// Ordinary registration always creates Worker mode and its own calendar.
    RegisterWorkerRequest { display_name: String, qualifications: Qualifications, calendar: CalendarDefinition });
dto!(/// The caller changes its own qualifications.
    UpdateQualificationsRequest { qualifications: Qualifications });
dto!(/// The caller changes its own active mode; promotion remains privileged.
    SetWorkerModeRequest { mode: WorkerMode });
dto!(/// System authority grants coordinator eligibility to this account.
    PromoteCoordinatorRequest { account: AccountId32 });
dto!(/// Create or rename a catalog entry; role 1 is protected by the provider.
    UpsertCatalogEntryRequest { kind: CatalogKind, id: u32, name: String });
dto!(/// Delete a catalog entry subject to provider reference/invariant checks.
    DeleteCatalogEntryRequest { kind: CatalogKind, id: u32 });
dto!(/// Dev-only mint, authorized against the configured system account.
    FundAccountRequest { account: AccountId32, amount: Money });

/// Configurable integer weights validated on construction and deserialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Encode)]
#[serde(try_from = "ScorePolicyWire", into = "ScorePolicyWire")]
pub struct ScorePolicy {
    coordinator_percent: Percentage,
    client_percent: Percentage,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScorePolicyWire {
    coordinator_percent: Percentage,
    client_percent: Percentage,
}
impl ScorePolicy {
    /// Percentages must sum to exactly 100.
    pub fn new(
        coordinator_percent: Percentage,
        client_percent: Percentage,
    ) -> Result<Self, ContractError> {
        if u16::from(coordinator_percent.get()) + u16::from(client_percent.get()) != 100 {
            return Err(ContractError::Invalid("score percentages"));
        }
        Ok(Self {
            coordinator_percent,
            client_percent,
        })
    }
    /// Coordinator contribution percentage.
    #[must_use]
    pub const fn coordinator_percent(self) -> Percentage {
        self.coordinator_percent
    }
    /// Client contribution percentage.
    #[must_use]
    pub const fn client_percent(self) -> Percentage {
        self.client_percent
    }
    /// Exact contribution in hundredths of a score point; no intermediate rounding.
    #[must_use]
    pub fn blend(self, coordinator: Score, client: Score) -> u16 {
        u16::from(coordinator.get()) * u16::from(self.coordinator_percent.get())
            + u16::from(client.get()) * u16::from(self.client_percent.get())
    }
}
impl TryFrom<ScorePolicyWire> for ScorePolicy {
    type Error = ContractError;
    fn try_from(value: ScorePolicyWire) -> Result<Self, Self::Error> {
        Self::new(value.coordinator_percent, value.client_percent)
    }
}
impl From<ScorePolicy> for ScorePolicyWire {
    fn from(value: ScorePolicy) -> Self {
        Self {
            coordinator_percent: value.coordinator_percent,
            client_percent: value.client_percent,
        }
    }
}
impl Decode for ScorePolicy {
    fn decode<I: parity_scale_codec::Input>(
        input: &mut I,
    ) -> Result<Self, parity_scale_codec::Error> {
        Self::new(Percentage::decode(input)?, Percentage::decode(input)?)
            .map_err(|_| "score percentages must sum to 100".into())
    }
}

dto!(/// Public work request; coordinator and client are derived, never supplied.
    CreateProjectRequest { title: String, description: String });
dto!(/// Fixed planning price and committed duration, proposed by its coordinator.
    PlanningQuote { fee: Money, minutes: Minutes, window: WeekWindow });
dto!(/// Revision observed by the client; checked atomically before acceptance effects.
    RevisionRequest { expected_revision: u64 });
dto!(/// One person/slot, with explicit compensation and all required skills.
    RequirementDefinition { key: u32, role_id: u32, skill_ids: Vec<u32>, minutes: Minutes, budget: Money });
dto!(/// One milestone quote with a provider-created attached task storage.
    MilestoneDefinition { key: u32, title: String, window: WeekWindow, coordinator_fee: Money, coordinator_minutes: Minutes, requirements: Vec<RequirementDefinition> });
dto!(/// Editable draft definition; resource identities are provider generated.
    ProposalDefinition { title: String, description: String, milestones: Vec<MilestoneDefinition> });
dto!(/// A changes request references the client's explanation.
    RequestChangesRequest { reference: String });
dto!(/// Unresolved cancellation or dispute explanation, without a payout instruction.
    ReasonRequest { reason: String });
dto!(/// Task content controlled by a coordinator, not a contractual assignment.
    TaskDefinition { title: String, description: String, task_type: TaskType, priority: TaskPriority, status: TaskStatus, assignees: Vec<AccountId32>, estimated_minutes: Minutes, logged_minutes: Minutes, due_at: Option<UnixSeconds> });
dto!(/// The only task changes allowed to an assignee.
    TaskProgressRequest { status: TaskStatus, logged_minutes: Minutes });
dto!(/// Coordinator rating of one assigned worker.
    WorkerRating { worker: AccountId32, score: Score });
dto!(/// Completion request includes each contractual worker's individual rating.
    RequestMilestoneCompletionRequest { worker_ratings: Vec<WorkerRating>, deliverable: EvidenceReference });

/// A client supplies one team score or delegates to individual coordinator scores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(tag = "type", content = "score", deny_unknown_fields)]
#[allow(clippy::cast_possible_truncation)] // SCALE derive indexes two variants.
pub enum TeamRating {
    /// Blend this client score with every coordinator worker rating.
    Client(Score),
    /// Use each coordinator worker rating without inventing a client vote.
    DelegateToCoordinator,
}
dto!(/// Accept completion and settle its quoted payments and scores atomically.
    AcceptMilestoneCompletionRequest { coordinator_score: Score, team_rating: TeamRating, submission_id: EntityId });

/// Closed provider command set. Targets and messages are implicit in the variant
/// and resource IDs; no browser endpoint accepts this enum as a generic command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(
    tag = "type",
    content = "data",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[allow(missing_docs, clippy::cast_possible_truncation)] // SCALE derive indexes 27 variants.
pub enum ProviderCommand {
    RegisterWorker(RegisterWorkerRequest),
    UpdateQualifications(UpdateQualificationsRequest),
    SetCalendar(CalendarDefinition),
    SetWorkerMode(SetWorkerModeRequest),
    PromoteCoordinator(PromoteCoordinatorRequest),
    UpsertCatalogEntry(UpsertCatalogEntryRequest),
    DeleteCatalogEntry(DeleteCatalogEntryRequest),
    FundAccount(FundAccountRequest),
    SetScorePolicy(ScorePolicy),
    CreateProject(CreateProjectRequest),
    QuotePlanning {
        project_id: EntityId,
        quote: PlanningQuote,
    },
    AcceptPlanningQuote {
        project_id: EntityId,
        expected_revision: u64,
    },
    CreateProposal {
        project_id: EntityId,
        proposal: ProposalDefinition,
    },
    UpdateProposal {
        project_id: EntityId,
        proposal_id: EntityId,
        proposal: ProposalDefinition,
    },
    DeleteProposal {
        project_id: EntityId,
        proposal_id: EntityId,
    },
    SubmitProposal {
        project_id: EntityId,
        proposal_id: EntityId,
    },
    AcceptPlanningDelivery {
        project_id: EntityId,
        expected_revision: u64,
    },
    ApproveExecution {
        project_id: EntityId,
        proposal_id: EntityId,
        expected_revision: u64,
    },
    RequestProposalChanges {
        project_id: EntityId,
        proposal_id: EntityId,
        request: RequestChangesRequest,
    },
    CancelProject {
        project_id: EntityId,
        request: ReasonRequest,
    },
    DisputePlanning {
        project_id: EntityId,
        request: ReasonRequest,
    },
    RejectMilestoneCompletion {
        project_id: EntityId,
        milestone_id: EntityId,
        submission_id: EntityId,
        request: EvidenceRequest,
    },
    OpenDispute(OpenDisputeRequest),
    RespondDispute {
        project_id: EntityId,
        dispute_id: EntityId,
        request: EvidenceRequest,
    },
    CreateTask {
        project_id: EntityId,
        task_storage_id: EntityId,
        task: TaskDefinition,
    },
    EditTask {
        project_id: EntityId,
        task_storage_id: EntityId,
        task_id: u32,
        task: TaskDefinition,
    },
    UpdateTaskProgress {
        project_id: EntityId,
        task_storage_id: EntityId,
        task_id: u32,
        progress: TaskProgressRequest,
    },
    RequestMilestoneCompletion {
        project_id: EntityId,
        milestone_id: EntityId,
        request: RequestMilestoneCompletionRequest,
    },
    AcceptMilestoneCompletion {
        project_id: EntityId,
        milestone_id: EntityId,
        request: AcceptMilestoneCompletionRequest,
    },
    CreateDeposit(CreateDepositRequest),
    ConfirmDeposit {
        deposit_id: EntityId,
    },
    CreateWithdrawal(CreateWithdrawalRequest),
    CancelWithdrawal {
        withdrawal_id: EntityId,
    },
}

fn nonempty(value: &str, field: &'static str) -> Result<(), ContractError> {
    if value.trim().is_empty() {
        Err(ContractError::Invalid(field))
    } else {
        Ok(())
    }
}
fn unique<T: Ord>(values: &[T], field: &'static str) -> Result<(), ContractError> {
    let distinct: std::collections::BTreeSet<_> = values.iter().collect();
    if distinct.len() == values.len() {
        Ok(())
    } else {
        Err(ContractError::Invalid(field))
    }
}
fn catalog_ids(values: &[u32], field: &'static str) -> Result<(), ContractError> {
    if values.contains(&0) {
        return Err(ContractError::Invalid(field));
    }
    unique(values, field)
}
impl Qualifications {
    /// Reject zero or duplicate catalog IDs; existence is checked by the provider.
    pub fn validate(&self) -> Result<(), ContractError> {
        catalog_ids(&self.role_ids, "role IDs")?;
        catalog_ids(&self.skill_ids, "skill IDs")
    }
}
impl CalendarDefinition {
    /// Multiple overrides for one week would have ambiguous meaning.
    pub fn validate(&self) -> Result<(), ContractError> {
        unique(
            &self
                .overrides
                .iter()
                .map(|item| item.week)
                .collect::<Vec<_>>(),
            "weekly overrides",
        )
    }
}
impl ProposalDefinition {
    /// Validate keys, requirements and overflow before quoting or signing.
    pub fn validate(&self) -> Result<(), ContractError> {
        nonempty(&self.title, "proposal title")?;
        if self.milestones.is_empty() {
            return Err(ContractError::Invalid("empty milestones"));
        }
        unique(
            &self
                .milestones
                .iter()
                .map(|item| item.key)
                .collect::<Vec<_>>(),
            "milestone keys",
        )?;
        for milestone in &self.milestones {
            nonempty(&milestone.title, "milestone title")?;
            unique(
                &milestone
                    .requirements
                    .iter()
                    .map(|item| item.key)
                    .collect::<Vec<_>>(),
                "requirement keys",
            )?;
            for requirement in &milestone.requirements {
                if requirement.role_id == 0 || requirement.skill_ids.is_empty() {
                    return Err(ContractError::Invalid("requirement role or skills"));
                }
                catalog_ids(&requirement.skill_ids, "requirement skills")?;
            }
            milestone.total()?;
        }
        self.total()?;
        Ok(())
    }
    /// Exact quoted total, failing before any balance mutation on overflow.
    pub fn total(&self) -> Result<Money, ContractError> {
        self.milestones.iter().try_fold(Money::ZERO, |total, item| {
            Ok(total.checked_add(item.total()?)?)
        })
    }
}
impl MilestoneDefinition {
    /// Coordinator fee plus each requirement budget, with checked arithmetic.
    pub fn total(&self) -> Result<Money, ContractError> {
        self.requirements
            .iter()
            .try_fold(self.coordinator_fee, |total, item| {
                Ok(total.checked_add(item.budget)?)
            })
    }
}
impl TaskDefinition {
    /// Reject blank titles and duplicate tracking assignees.
    pub fn validate(&self) -> Result<(), ContractError> {
        nonempty(&self.title, "task title")?;
        unique(&self.assignees, "task assignees")
    }
}
impl ProviderCommand {
    /// Validate all structural constraints; authorization and current-state
    /// invariants must still be enforced in the provider transaction.
    pub fn validate(&self) -> Result<(), ContractError> {
        match self {
            Self::RegisterWorker(request) => {
                nonempty(&request.display_name, "worker display name")?;
                request.qualifications.validate()?;
                request.calendar.validate()
            }
            Self::UpdateQualifications(request) => request.qualifications.validate(),
            Self::SetCalendar(calendar) => calendar.validate(),
            Self::UpsertCatalogEntry(request) => {
                if request.id == 0 {
                    return Err(ContractError::Invalid("catalog ID"));
                }
                nonempty(&request.name, "catalog name")
            }
            Self::DeleteCatalogEntry(request) if request.id == 0 => {
                Err(ContractError::Invalid("catalog ID"))
            }
            Self::CreateProject(request) => nonempty(&request.title, "project title"),
            Self::CreateProposal { proposal, .. } | Self::UpdateProposal { proposal, .. } => {
                proposal.validate()
            }
            Self::RequestProposalChanges { request, .. } => {
                nonempty(&request.reference, "change reference")
            }
            Self::CancelProject { request, .. } | Self::DisputePlanning { request, .. } => {
                nonempty(&request.reason, "reason")
            }
            Self::CreateTask { task, .. } | Self::EditTask { task, .. } => task.validate(),
            Self::RequestMilestoneCompletion { request, .. } => unique(
                &request
                    .worker_ratings
                    .iter()
                    .map(|item| item.worker)
                    .collect::<Vec<_>>(),
                "worker ratings",
            ),
            _ => Ok(()),
        }
    }
    /// Target project when this command belongs to an existing project.
    #[must_use]
    pub const fn project_id(&self) -> Option<EntityId> {
        match self {
            Self::OpenDispute(request) => Some(request.project_id),
            Self::QuotePlanning { project_id, .. }
            | Self::AcceptPlanningQuote { project_id, .. }
            | Self::CreateProposal { project_id, .. }
            | Self::UpdateProposal { project_id, .. }
            | Self::DeleteProposal { project_id, .. }
            | Self::SubmitProposal { project_id, .. }
            | Self::AcceptPlanningDelivery { project_id, .. }
            | Self::ApproveExecution { project_id, .. }
            | Self::RequestProposalChanges { project_id, .. }
            | Self::CancelProject { project_id, .. }
            | Self::DisputePlanning { project_id, .. }
            | Self::RejectMilestoneCompletion { project_id, .. }
            | Self::RespondDispute { project_id, .. }
            | Self::CreateTask { project_id, .. }
            | Self::EditTask { project_id, .. }
            | Self::UpdateTaskProgress { project_id, .. }
            | Self::RequestMilestoneCompletion { project_id, .. }
            | Self::AcceptMilestoneCompletion { project_id, .. } => Some(*project_id),
            _ => None,
        }
    }
}

dto!(/// Exact signed content; variant plus IDs encodes the target and message.
    UnsignedContractCallV1 { signing_domain: [u8; 16], provider_instance_id: ProviderInstanceId, payload_version: u16, operation_id: OperationId, origin: AccountId32, nonce: u64, expires_at: UnixSeconds, command: ProviderCommand });
dto!(/// Unchanged unsigned content accompanied by its fixed-width signature.
    SignedContractCallV1 { call: UnsignedContractCallV1, signature: Sr25519Signature });
impl UnsignedContractCallV1 {
    /// Construct the supported format, validating the command before use.
    pub fn new(
        provider_instance_id: ProviderInstanceId,
        operation_id: OperationId,
        origin: AccountId32,
        nonce: u64,
        expires_at: UnixSeconds,
        command: ProviderCommand,
    ) -> Result<Self, ContractError> {
        let call = Self {
            signing_domain: MOCK_SIGNING_DOMAIN,
            provider_instance_id,
            payload_version: PAYLOAD_VERSION,
            operation_id,
            origin,
            nonce,
            expires_at,
            command,
        };
        call.validate()?;
        Ok(call)
    }
    /// Check protocol and command constraints, not runtime expiration or nonce.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.signing_domain != MOCK_SIGNING_DOMAIN || self.payload_version != PAYLOAD_VERSION {
            return Err(ContractError::InvalidPayload);
        }
        self.command.validate()?;
        if self.encoded_size() > MAX_SIGNABLE_BYTES {
            return Err(ContractError::InvalidPayload);
        }
        Ok(())
    }
    /// These exact bytes are hashed, persisted, and signed without a JSON step.
    pub fn signable_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        Ok(self.encode())
    }
    /// Decode one complete bounded SCALE call and validate nested constraints.
    pub fn decode_signable(bytes: &[u8]) -> Result<Self, ContractError> {
        if bytes.is_empty() || bytes.len() > MAX_SIGNABLE_BYTES {
            return Err(ContractError::InvalidPayload);
        }
        let mut input = bytes;
        let call = Self::decode(&mut input).map_err(|_| ContractError::InvalidPayload)?;
        if !input.is_empty() {
            return Err(ContractError::InvalidPayload);
        }
        call.validate()?;
        Ok(call)
    }
}

dto!(/// Public, read-only custody metadata; the system wallet has no principal.
    WalletView { wallet_id: WalletId, principal_id: Option<PrincipalId>, account_id: AccountId32, lifecycle: WalletLifecycle });
dto!(/// Human-wallet provisioning; system provisioning is not caller-selectable.
    ProvisionWalletRequest { principal_id: PrincipalId });
dto!(/// Internal immutable signing request, authenticated by service credential.
    CreateSigningJobRequest { operation_id: OperationId, wallet_id: WalletId, payload_version: u16, signable_payload: Vec<u8>, payload_hash: PayloadHash, expires_at: UnixSeconds });
dto!(/// Durable signing result. Signature exists only when status is Signed.
    SigningJobView { operation_id: OperationId, wallet_id: WalletId, account_id: AccountId32, status: SigningJobStatus, signature: Option<Sr25519Signature>, rejection_code: Option<String> });
dto!(/// Provider generation and privileged public account, never a secret.
    ProviderInfo { provider_instance_id: ProviderInstanceId, root_account: AccountId32, payload_version: u16 });
dto!(/// Next nonce for new execution, not for an authenticated replay.
    AccountNonce { account: AccountId32, nonce: u64 });

/// Execution result is independent from finality or transport acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(tag = "type", content = "code", deny_unknown_fields)]
#[allow(clippy::cast_possible_truncation)] // SCALE derive indexes two variants.
pub enum ExecutionOutcome {
    /// Business operation committed successfully.
    Success,
    /// Finalized execution failed without committing partial business effects.
    Failed(String),
}
dto!(/// Recoverable provider execution receipt; generated IDs are not inferred.
    OperationReceipt { operation_id: OperationId, provider_instance_id: ProviderInstanceId, origin: AccountId32, nonce: u64, outcome: ExecutionOutcome, created_entity_id: Option<EntityId>, first_event_cursor: Option<u64>, last_event_cursor: Option<u64>, finalized_at: UnixSeconds });
dto!(/// Browser mutation acknowledgement, not a claim of business success.
    OperationRef { operation_id: OperationId, status: ProviderOperationStatus });
dto!(/// Browser-pollable outcome, retained independently of an SSE connection.
    OperationView { operation_id: OperationId, status: ProviderOperationStatus, receipt: Option<OperationReceipt>, error_code: Option<String> });
dto!(/// Stable sanitized HTTP error for internal and browser boundaries.
    ApiError { code: String, message: String });

redacted_dto!(/// Classic registration does not contain worker mode or privilege claims.
    RegisterRequest { username: String, password: String, display_name: String });
redacted_dto!(/// Credentials must never be logged or derive wallet secrets.
    LoginRequest { username: String, password: String });
redacted_dto!(/// Password replacement changes credentials, not account or wallet identity.
    ChangePasswordRequest { current_password: String, new_password: String });
redacted_dto!(/// Current session information; cookie is `HttpOnly`, CSRF token is client-visible.
    SessionView { principal_id: PrincipalId, account_id: AccountId32, display_name: String, csrf_token: String, is_admin: bool });

dto!(/// Editable role/skill seed entry; protected entries expose their restriction.
    CatalogEntry { id: u32, name: String, fixed: bool });
dto!(/// Provider-owned role/skill catalogs and score policy.
    CatalogView { roles: Vec<CatalogEntry>, skills: Vec<CatalogEntry>, score_policy: ScorePolicy });
dto!(/// Exact reputation numerator is hundredths-of-score times committed minutes.
    ReputationView { #[serde(with = "decimal_u128")] weighted_score_sum: u128, rated_minutes: u64 });
dto!(/// One committed weekly allocation, retained after completion.
    ReservationView { project_id: EntityId, milestone_id: Option<EntityId>, requirement_key: Option<u32>, week: Week, minutes: Minutes });
dto!(/// Calendar instance owned by exactly one worker account.
    CalendarView { calendar_id: EntityId, owner: AccountId32, definition: CalendarDefinition, reservations: Vec<ReservationView> });
dto!(/// Worker public read model with distinct mode-specific reputations.
    WorkerView { account: AccountId32, display_name: String, qualifications: Qualifications, mode: WorkerMode, coordinator_eligible: bool, calendar: CalendarView, worker_score: ReputationView, coordinator_score: ReputationView });
dto!(/// Aggregated committed minutes for a week, without project or milestone IDs.
    WeeklyCommitment { week: Week, minutes: Minutes });
dto!(/// Public capacity and checked weekly commitment totals, without reservation identities.
    CalendarSummaryView { calendar_id: EntityId, owner: AccountId32, definition: CalendarDefinition, committed_minutes: Vec<WeeklyCommitment> });
dto!(/// Public worker directory without another project's reservation identifiers.
    WorkerSummaryView { account: AccountId32, display_name: String, qualifications: Qualifications, mode: WorkerMode, coordinator_eligible: bool, calendar: CalendarSummaryView, worker_score: ReputationView, coordinator_score: ReputationView });
dto!(/// One requirement's contractual assignment, not a tracking task assignee.
    AssignmentView { requirement_key: u32, worker: AccountId32 });
dto!(/// Provider-owned task key, reporter and timestamps cannot be supplied in edits.
    TaskView { task_id: u32, reporter: AccountId32, created_at: UnixSeconds, updated_at: UnixSeconds, task: TaskDefinition });
dto!(/// Storage created and attached atomically with its milestone.
    TaskStorageView { task_storage_id: EntityId, milestone_id: EntityId, tasks: Vec<TaskView> });
dto!(/// Quote, execution state, contractual workers and attached tracking storage.
    MilestoneView { milestone_id: EntityId, definition: MilestoneDefinition, status: Option<MilestoneStatus>, assignments: Vec<AssignmentView>, worker_ratings: Vec<WorkerRating>, task_storage: TaskStorageView, frozen: bool, submissions: Vec<CompletionSubmission> });
dto!(/// Proposal read model; its total is derived from checked quote line items.
    ProposalView { proposal_id: EntityId, revision: u64, title: String, description: String, status: ProposalStatus, milestones: Vec<MilestoneView>, change_request: Option<String> });
dto!(/// Negotiation and settlement of planning, separate from execution.
    PlanningView { revision: u64, status: PlanningStatus, quote: Option<PlanningQuote>, escrow: Money, frozen: bool });
dto!(/// Project read model; completion follows acceptance of the final funded milestone.
    ProjectView { project_id: EntityId, client: AccountId32, coordinator: AccountId32, title: String, description: String, planning: PlanningView, proposals: Vec<ProposalView>, execution_escrow: Money, cancelled: bool, completed: bool, active_dispute_id: Option<EntityId> });
dto!(/// Available balance for the initial KVN asset, excluding locked escrow.
    BalanceView { account: AccountId32, asset_id: u32, available: Money });
dto!(/// Internal read-only snapshot. Adapter filters confidential project/task data.
    ProviderSnapshot { info: ProviderInfo, catalog: CatalogView, workers: Vec<WorkerView>, projects: Vec<ProjectView>, balances: Vec<BalanceView> });

wire_enum!(/// Durable state-change event vocabulary, committed with its command.
    DomainEventKind { WorkerRegistered, WorkerUpdated, CalendarUpdated, CatalogUpdated, CoordinatorPromoted, ScorePolicyUpdated, AccountFunded, ProjectCreated, PlanningQuoted, PlanningAccepted, ProposalCreated, ProposalUpdated, ProposalDeleted, ProposalSubmitted, PlanningCompleted, ExecutionApproved, ProposalChangesRequested, ProjectCancelled, PlanningDisputed, MilestoneDisputed, TaskCreated, TaskUpdated, MilestoneCompletionRequested, MilestoneCompleted, MilestoneCompletionRejected, DisputeOpened, DisputeResponseAdded, DepositRequested, DepositConfirmed, WithdrawalRequested, WithdrawalCancelled });
dto!(/// Durable provider event with explicit recipients, no secret or raw payload.
    DomainEvent { provider_instance_id: ProviderInstanceId, cursor: u64, operation_id: OperationId, kind: DomainEventKind, project_id: Option<EntityId>, entity_id: Option<EntityId>, recipients: Vec<AccountId32>, occurred_at: UnixSeconds });
dto!(/// Cursor-based provider event page; cursor always refers to this instance.
    ProviderEvents { provider_instance_id: ProviderInstanceId, events: Vec<DomainEvent>, next_cursor: u64 });
dto!(/// Session-owned persisted notification; SSE delivery never changes `read_at`.
    NotificationView { notification_id: u64, event: DomainEvent, read_at: Option<UnixSeconds> });
dto!(/// Public notification page, resumable by its monotonically increasing ID.
    NotificationsPage { notifications: Vec<NotificationView>, next_cursor: u64 });

/// An immutable validated call, ready for hashing and signing. This construction
/// state exposes no mutable command fields and only transitions to signed output.
#[derive(Clone)]
pub struct PreparedCall {
    call: UnsignedContractCallV1,
    bytes: Vec<u8>,
}
impl PreparedCall {
    /// Freeze a structurally valid call and its exact SCALE representation.
    pub fn new(call: UnsignedContractCallV1) -> Result<Self, ContractError> {
        let bytes = call.signable_bytes()?;
        Ok(Self { call, bytes })
    }
    /// Exact immutable bytes to hash and sign.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Public metadata remains read-only while awaiting signature.
    #[must_use]
    pub const fn call(&self) -> &UnsignedContractCallV1 {
        &self.call
    }
    /// Consume the unsigned state and bind a signature. Cryptographic signature
    /// verification remains mandatory at the provider trust boundary.
    #[must_use]
    pub fn with_signature(self, signature: Sr25519Signature) -> SignedContractCallV1 {
        SignedContractCallV1 {
            call: self.call,
            signature,
        }
    }
}

#[cfg(test)]
mod tests;
