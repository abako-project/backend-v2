//! Consumer forms for future adapter-owned task operations. Current task writers
//! still produce provider receipts and events; these types do not enable cutover.

use crate::{
    AccountId32, DomainEvent, DomainEventKind, EntityId, ExecutionOutcome, OperationId,
    ProviderInstanceId, UnixSeconds,
};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};

dto!(/// API task completion, without signing metadata or a provider event cursor.
    TaskOperationResult { outcome: ExecutionOutcome, task_storage_id: EntityId, finalized_at: UnixSeconds });

wire_enum!(/// Explicit API ownership; a generation ID alone does not establish it.
    TaskEventSource { AdapterTask });
wire_enum!(/// Only tracking changes can be represented as API task notifications.
    TaskEventKind { TaskCreated, TaskUpdated, TaskDeleted });
dto!(/// Generation-scoped API task event. Notification IDs provide SSE ordering.
    AdapterTaskEvent { source: TaskEventSource, provider_instance_id: ProviderInstanceId, operation_id: OperationId, kind: TaskEventKind, project_id: EntityId, task_storage_id: EntityId, recipients: Vec<AccountId32>, occurred_at: UnixSeconds, origin: AccountId32 });

/// Exactly one event source. The provider variant retains its original JSON.
/// Required `source` and strict field sets reject mixed or unknown event shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(untagged)]
#[allow(clippy::cast_possible_truncation)] // SCALE derive indexes two variants.
pub enum NotificationEvent {
    /// Existing signed provider event, including its real provider cursor.
    Provider(DomainEvent),
    /// API-only task change, without an invented provider event cursor.
    Task(AdapterTaskEvent),
}

impl NotificationEvent {
    /// Generation that scopes the event's entity IDs.
    #[must_use]
    pub const fn provider_instance_id(&self) -> ProviderInstanceId {
        match self {
            Self::Provider(event) => event.provider_instance_id,
            Self::Task(event) => event.provider_instance_id,
        }
    }

    /// Public operation reference, independent of execution authority.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        match self {
            Self::Provider(event) => event.operation_id,
            Self::Task(event) => event.operation_id,
        }
    }

    /// Existing display vocabulary; this does not convert API events to provider events.
    #[must_use]
    pub const fn kind(&self) -> DomainEventKind {
        match self {
            Self::Provider(event) => event.kind,
            Self::Task(event) => match event.kind {
                TaskEventKind::TaskCreated => DomainEventKind::TaskCreated,
                TaskEventKind::TaskUpdated => DomainEventKind::TaskUpdated,
                TaskEventKind::TaskDeleted => DomainEventKind::TaskDeleted,
            },
        }
    }

    /// Project for visibility-checked enrichment.
    #[must_use]
    pub const fn project_id(&self) -> Option<EntityId> {
        match self {
            Self::Provider(event) => event.project_id,
            Self::Task(event) => Some(event.project_id),
        }
    }

    /// Verified actor; historical provider records may lack one.
    #[must_use]
    pub const fn origin(&self) -> Option<AccountId32> {
        match self {
            Self::Provider(event) => event.origin,
            Self::Task(event) => Some(event.origin),
        }
    }
}
