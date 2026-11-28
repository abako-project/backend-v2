//! Reads one Project: its tree through the contract's `view` message, and its
//! parent, header, reasons and disputes from their storage entries.
use crate::{
    Account, Error, MAX_SHELL,
    facts::{Reason, Storage},
};
use parity_scale_codec::{Decode, Encode};
use protocol::{provider_core::*, *};
type Result<T> = std::result::Result<T, Error>;
macro_rules! record {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Clone, Encode, Decode)]
        pub(crate) struct $name { $(pub(crate) $field: $ty,)* }
    };
}
record!(ProjectRecord {
    project_id: EntityId, created_at: Option<UnixSeconds>, client: AccountId32, coordinator: AccountId32,
    content: MetadataReferenceV1, planning: PlanningView, proposals: u32,
    execution_escrow: Money, cancelled: bool, completed: bool, evaluations: u32,
    active_dispute_id: Option<EntityId>, reasons: u32, disputes: u32
});
fn items<T>(length: u32, read: impl FnMut(u32) -> Result<T>) -> Result<Vec<T>> {
    if length as usize > MAX_SHELL {
        return Err(Error::Limit);
    }
    (0..length).map(read).collect()
}
pub(crate) struct StoredProject {
    pub(crate) dao: Account,
    pub(crate) project: ProjectCore,
    pub(crate) reasons: Vec<Reason>,
    pub(crate) cases: Vec<Dispute>,
}
pub(crate) fn read(
    reader: &mut Storage<'_, '_>,
    address: &Account,
    origin: &Account,
) -> Result<StoredProject> {
    let (dao, id): (Account, EntityId) = reader.entry(address, 0u32)?;
    let record: ProjectRecord = reader.entry(address, 1u32)?;
    let project: ProjectCore = reader.message(address, origin, "view", ())?;
    let count = |length: usize| u32::try_from(length).map_err(|_| Error::Limit);
    if record.project_id != id
        || project.project_id != id
        || count(project.proposals.len())? != record.proposals
        || count(project.evaluations.len())? != record.evaluations
    {
        return Err(Error::InvalidState);
    }
    Ok(StoredProject {
        dao,
        project,
        reasons: items(record.reasons, |i| reader.value(address, 22, i))?,
        cases: items(record.disputes, |i| reader.value(address, 23, i))?,
    })
}
