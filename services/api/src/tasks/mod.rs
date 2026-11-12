//! API-owned task content, contract membership confirmation and dispute holds.
//! Execution uses the existing operation queue; store writes stay in its transaction.
pub(crate) mod execution;
pub(crate) mod freezes;
pub(crate) mod history;
pub(crate) mod materialization;
pub(crate) mod state;
mod store;
pub(crate) mod views;
