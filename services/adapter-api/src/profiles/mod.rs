//! Adapter-owned descriptive profiles; provider-owned qualifications stay elsewhere.

mod http;
mod models;
mod store;

pub(crate) use http::{get_me, get_public, put_me};

#[cfg(test)]
mod tests;
