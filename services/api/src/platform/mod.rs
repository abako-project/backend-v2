use std::{fs::File, io::Read, path::Path, sync::Arc};

use axum::{Json, extract::State};
use protocol::PlatformProfile;

use crate::state::{App, Error};

const MAX_CONFIG_BYTES: u64 = 64 * 1024;
const DEFAULT_PROFILE: &str = include_str!("default.json");
const LEGACY_PROJECT_TYPES: [&str; 6] = [
    "Other",
    "SmartContract",
    "Frontend",
    "MVP",
    "Audit",
    "MobileApp",
];

pub(crate) fn load(path: Option<&Path>) -> Result<PlatformProfile, Error> {
    let bytes = match path {
        Some(path) => {
            let mut bytes = Vec::new();
            File::open(path)
                .map_err(|_| Error::Config)?
                .take(MAX_CONFIG_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Config)?;
            bytes
        }
        None => DEFAULT_PROFILE.as_bytes().to_vec(),
    };
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(Error::Config);
    }
    let profile: PlatformProfile = serde_json::from_slice(&bytes).map_err(|_| Error::Config)?;
    profile.validate().map_err(|_| Error::Config)?;
    Ok(profile)
}

pub(crate) fn allows_project_type(profile: &PlatformProfile, value: &str) -> bool {
    LEGACY_PROJECT_TYPES.contains(&value) || profile.project_types.iter().any(|item| item == value)
}

pub(crate) async fn get(State(app): State<Arc<App>>) -> Json<PlatformProfile> {
    Json(app.config.platform.clone())
}

#[cfg(test)]
mod tests;
