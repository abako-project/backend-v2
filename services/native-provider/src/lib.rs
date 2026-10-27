//! Disposable smart-contract simulator with one atomic command boundary.
#![allow(clippy::missing_errors_doc)]

mod calendar;
pub mod creation;
mod domain;
pub mod proposal;
mod provider_api;
mod seed;
mod storage;

pub use provider::{Error, ProviderApi, Result, router};
pub use storage::Provider;

pub(crate) fn require(condition: bool, code: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::domain(code))
    }
}

/// Explicit compact catalog/reason legacy conversion. No preimages are stored.
pub mod catalog_content;

/// Hash exact descriptive content bytes for the adapter commitment.
#[must_use]
pub fn content_hash(bytes: &[u8]) -> protocol::PayloadHash {
    use sha2::{Digest, Sha256};
    protocol::PayloadHash::from_bytes(Sha256::digest(bytes).into())
}
