//! Immutable, offline artifact cohort. No caller-supplied WASM enters the worker.
use crate::ProofError;
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode};
use sp_core::hashing::{blake2_256, sha2_256};
use std::collections::BTreeMap;

const MANIFEST: &[u8] = include_bytes!("../fixtures/manifest.json");
const ARTIFACTS: [(&str, &[u8], &[u8]); 4] = [
    (
        "projects-v5",
        include_bytes!("../fixtures/projects-v5.contract"),
        include_bytes!("../fixtures/build-locks/projects-v5.lock"),
    ),
    (
        "directory-v5",
        include_bytes!("../fixtures/directory-v5.contract"),
        include_bytes!("../fixtures/build-locks/directory-v5.lock"),
    ),
    (
        "calendar-v5",
        include_bytes!("../fixtures/calendar-v5.contract"),
        include_bytes!("../fixtures/build-locks/calendar-v5.lock"),
    ),
    (
        "sandbox-probe",
        include_bytes!("../fixtures/sandbox-probe.contract"),
        include_bytes!("../fixtures/build-locks/sandbox-probe.lock"),
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking)]
pub struct Identity {
    pub runtime: [u8; 32],
    pub bundles: [u8; 32],
}
impl Identity {
    pub fn pinned() -> Self {
        // Changes require an explicit import, rather than reinterpret existing state.
        // Including complete sources is deliberately conservative for this candidate.
        let runtime = [
            include_bytes!("lib.rs").as_slice(),
            include_bytes!("assets.rs").as_slice(),
            include_bytes!("../Cargo.lock").as_slice(),
        ]
        .concat();
        Self {
            runtime: sha2_256(&runtime),
            bundles: sha2_256(MANIFEST),
        }
    }
}

pub struct Bundles {
    identity: Identity,
    code: BTreeMap<&'static str, Vec<u8>>,
}
impl Bundles {
    pub fn pinned() -> Result<Self, ProofError> {
        let manifest: serde_json::Value =
            serde_json::from_slice(MANIFEST).map_err(|_| ProofError::Bundle)?;
        if manifest["schemaVersion"] != 1 || manifest["contractToolchain"]["ink"] != "5.1.1" {
            return Err(ProofError::Bundle);
        }
        let mut code = BTreeMap::new();
        for (name, artifact, lock) in ARTIFACTS {
            code.insert(name, verify(artifact, lock, &manifest["contracts"][name])?);
        }
        Ok(Self {
            identity: Identity::pinned(),
            code,
        })
    }
    pub fn identity(&self) -> Identity {
        self.identity
    }
    pub fn code(&self, name: &str) -> Result<&[u8], ProofError> {
        self.code
            .get(name)
            .map(Vec::as_slice)
            .ok_or(ProofError::Bundle)
    }
}
fn hex(value: &str) -> Result<Vec<u8>, ProofError> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    if !value.len().is_multiple_of(2) || !value.is_ascii() {
        return Err(ProofError::Bundle);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|part| {
            let text = std::str::from_utf8(part).map_err(|_| ProofError::Bundle)?;
            u8::from_str_radix(text, 16).map_err(|_| ProofError::Bundle)
        })
        .collect()
}
fn field<'a>(value: &'a serde_json::Value, key: &str) -> Result<&'a str, ProofError> {
    value[key].as_str().ok_or(ProofError::Bundle)
}
fn verify(artifact: &[u8], lock: &[u8], entry: &serde_json::Value) -> Result<Vec<u8>, ProofError> {
    if artifact.len() > 1024 * 1024
        || sha2_256(artifact).as_slice() != hex(field(entry, "artifactSha256")?)?
        || sha2_256(lock).as_slice() != hex(field(entry, "lockSha256")?)?
    {
        return Err(ProofError::Bundle);
    }
    let bundle: serde_json::Value =
        serde_json::from_slice(artifact).map_err(|_| ProofError::Bundle)?;
    if bundle["source"]["language"] != "ink! 5.1.1" {
        return Err(ProofError::Bundle);
    }
    let wasm = hex(field(&bundle["source"], "wasm")?)?;
    if wasm.len() > 192 * 1024
        || blake2_256(&wasm).as_slice() != hex(field(entry, "wasmBlake2b256")?)?
        || blake2_256(&wasm).as_slice() != hex(field(&bundle["source"], "hash")?)?
    {
        return Err(ProofError::Bundle);
    }
    Ok(wasm)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_artifact_or_build_lock_changes() {
        let manifest: serde_json::Value = serde_json::from_slice(MANIFEST).unwrap();
        let (name, original, lock) = ARTIFACTS[0];
        let mut changed = original.to_vec();
        changed[10] ^= 1;
        assert!(verify(&changed, lock, &manifest["contracts"][name]).is_err());
        assert!(verify(original, b"wrong lock", &manifest["contracts"][name]).is_err());
        assert_eq!(Bundles::pinned().unwrap().identity(), Identity::pinned());
    }
}
