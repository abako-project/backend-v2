//! The provider's private interface, served from a fork of Kreivo.
//!
//! The embedded contract provider runs Dao and Project in its own runtime.
//! This crate answers the same private interface from the contracts on a
//! Kreivo fork, through the `kreivo` crate, so the API's calls and its
//! operation pipeline do not change. The command routing, the fact decoding
//! and the signed-operation shell are the provider's.
#![cfg(feature = "fork")]

mod facts;
pub mod passkeys;
mod project_storage;
mod provider;
mod reads;
mod routing;
pub mod transport;
mod wallet;

pub use kreivo::{Enrollment, Key, PasskeyAssertion, PasskeyRegistration, passkey_device};
pub use provider::{Config, PasskeyLogin, Provider};

use serde::{Deserialize, Serialize};
use transport::execution::Shell;

/// An account on the chain.
pub(crate) type Account = protocol::AccountId32;

/// Most bytes one read may decode, and the most the saved shell may take.
pub(crate) const MAX_SHELL: usize = 64 * 1024 * 1024;

/// Why the provider could not answer.
#[derive(Debug)]
pub enum Error {
    /// The chain could not be reached, or refused.
    Chain(kreivo::Error),
    /// The saved state could not be read or written.
    Io(std::io::Error),
    /// A resource limit.
    Limit,
    /// The contracts or the saved state hold something this code cannot read.
    InvalidState,
    /// A named broken expectation.
    Invariant(&'static str),
    /// The signed-operation shell refused.
    Shell(transport::execution::Error),
    /// A command this provider does not route.
    IncompleteCoverage,
    /// A read's own refusal, with its status.
    Provider(transport::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Chain(error) => write!(f, "chain: {error}"),
            Self::Io(error) => write!(f, "saved state: {error}"),
            Self::Limit => f.write_str("resource limit"),
            Self::InvalidState => f.write_str("invalid state"),
            Self::Invariant(code) => f.write_str(code),
            Self::Shell(error) => f.write_str(error.code()),
            Self::IncompleteCoverage => f.write_str("incomplete contract coverage"),
            Self::Provider(_) => f.write_str("provider read failure"),
        }
    }
}

impl std::error::Error for Error {}

impl From<kreivo::Error> for Error {
    fn from(error: kreivo::Error) -> Self {
        Self::Chain(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<transport::execution::Error> for Error {
    fn from(error: transport::execution::Error) -> Self {
        Self::Shell(error)
    }
}

/// What the provider keeps beside the fork's own state.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct State {
    /// Layout version of this record.
    pub(crate) version: u32,
    /// Nonces, receipts and events.
    pub(crate) shell: Shell,
    /// The Dao contract.
    pub(crate) dao: Account,
    /// The sessions the provider opened and has not ended: account, session
    /// key's account, and when it ends in milliseconds since the Unix epoch.
    /// The keys themselves are never saved.
    #[serde(default)]
    pub(crate) sessions: Vec<(Account, Account, u64)>,
    /// The ID of the community set up at the first start, if there is one.
    #[serde(default)]
    pub(crate) community: Option<u16>,
}
