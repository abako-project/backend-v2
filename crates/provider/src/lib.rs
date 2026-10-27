//! Shared provider execution and authenticated HTTP interface.

mod api;
mod error;
pub mod execution;
mod http;

pub use api::ProviderApi;
pub use error::{Error, Result};
pub use http::router;
