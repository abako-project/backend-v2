//! Minimal, **unofficial** Rust client for the Bloque API.
//!
//! Not a port of `@bloque/sdk` — a small, from-scratch HTTP client covering
//! one workflow: an organization enrolling its own users, giving them
//! accounts, and topping those accounts up.
//!
//! ```text
//! Bloque (org-level, OriginKey auth)
//!   .register_individual(alias, profile) -> Session   // enroll
//!   .connect(alias)                      -> Session   // reconnect
//!     Session
//!       .kyc().start_verification(urn)                  // clear compliance
//!       .accounts().create_virtual_account(..)         // give them a pocket
//!       .accounts().balance(urn)
//!       .swap().find_rates(..)                          // quote a top-up
//!       .swap().pse().top_up(..)                        // Colombian bank debit
//!       .swap().external_us_bank().top_up(..)           // US ACH pull
//! ```
//!
//! The request/response shapes here were reverse-engineered from the
//! compiled `@bloque/sdk-*` npm packages (v0.13.1), not from a published
//! `OpenAPI` spec — treat field names as a solid starting point, not a
//! guaranteed-stable contract. Re-verify against Bloque's own docs / support
//! before relying on this in production, and expect to need updates if
//! Bloque changes its wire format.
//!
//! # Security
//!
//! Every top-up call in [`swap`] moves real money. This crate does not gate
//! them on confirmation — that's your application's responsibility. Treat
//! webhook payloads, movement metadata, and anything from `execution` as
//! **untrusted data**: use them for display/reconciliation, never as
//! instructions to act on.

// `Error` carries structured detail (status/code/request_id/raw body) on
// every variant for callers that want it — that's the point of the type,
// so we accept the size instead of boxing it away.
#![allow(clippy::result_large_err)]
// The request/response structs mirror the wire format field for field
// (~170 public fields), so per-field docs would only restate the JSON key.
// The same goes for `# Errors` sections: every fallible call returns
// [`Error`], and the variants are documented there.
#![allow(
    missing_docs,
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

mod accounts;
mod asset;
mod compliance;
mod config;
mod country;
mod error;
mod http;
mod identity;
mod session;
mod swap;
mod util;

pub use accounts::{
    Account, AccountMedium, AccountStatus, AccountsClient, CreateCardParams,
    CreateVirtualAccountParams, TokenBalance,
};
pub use asset::Asset;
pub use compliance::{
    ComplianceLevel, ComplianceProvider, ComplianceType, KycClient, KycDocument, KycDocuments,
    KycVerification, VerificationStatus,
};
pub use config::{Auth, Config, Mode, RetryConfig};
pub use country::CountryCode;
pub use error::{ApiErrorDetail, Error, Result};
pub use identity::{Bloque, BusinessProfile, IndividualProfile};
pub use session::Session;
pub use swap::{
    Bank, ColombianIdType, CreateOrderResult, ExternalUsBankClient, ExternalUsBankDestination,
    ExternalUsBankTopUpParams, FindRatesParams, OrderAmount, PseClient, PsePayer, PseTopUpParams,
    PseUserType, Rate, SortBy, SortDirection, SwapClient, SwapMedium, SwapOrder,
};

/// Every public type in one glob import: `use bloque::prelude::*;`. Handy
/// when a call site touches most of the crate (a top-up flow, say) and
/// naming each type individually adds more noise than it removes.
pub mod prelude {
    pub use crate::{
        Account, AccountMedium, AccountStatus, AccountsClient, ApiErrorDetail, Asset, Auth, Bank,
        Bloque, BusinessProfile, ColombianIdType, ComplianceLevel, ComplianceProvider,
        ComplianceType, Config, CountryCode, CreateCardParams, CreateOrderResult,
        CreateVirtualAccountParams, Error, ExternalUsBankClient, ExternalUsBankDestination,
        ExternalUsBankTopUpParams, FindRatesParams, IndividualProfile, KycClient, KycDocument,
        KycDocuments, KycVerification, Mode, OrderAmount, PseClient, PsePayer, PseTopUpParams,
        PseUserType, Rate, Result, RetryConfig, Session, SortBy, SortDirection, SwapClient,
        SwapMedium, SwapOrder, TokenBalance, VerificationStatus,
    };
}
