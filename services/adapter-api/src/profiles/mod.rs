//! Adapter-owned descriptive profiles; provider-owned qualifications stay elsewhere.

mod http;
mod media;
pub(crate) mod models;
pub(crate) mod store;

pub(crate) use http::{get_me, get_public, put_me};
pub(crate) use media::MAX_IMAGE_BYTES;
pub(crate) use media::{get_image, put_image};

#[cfg(test)]
mod tests;
