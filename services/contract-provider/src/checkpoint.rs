//! Bounded raw-trie checkpoint, including every contract child trie.
use crate::{ProofError, bundle::Identity};
use parity_scale_codec::{Decode, DecodeWithMemLimit, DecodeWithMemTracking, Encode};
use sp_core::{
    H256,
    hashing::{blake2_256, sha2_256},
    storage::{ChildInfo, well_known_keys::DEFAULT_CHILD_STORAGE_KEY_PREFIX},
};
use sp_runtime::{
    StateVersion,
    traits::{BlakeTwo256, Hash},
};
use sp_state_machine::{Backend, IterArgs};

pub const MAX_CHECKPOINT: usize = 64 * 1024 * 1024;
type RawNodes = Vec<(Vec<u8>, (Vec<u8>, i32))>;

#[derive(Clone, Encode, Decode, DecodeWithMemTracking)]
pub struct Checkpoint {
    version: u32,
    identity: Identity,
    root: H256,
    nodes: RawNodes,
}
impl Checkpoint {
    pub fn capture(mut ext: sp_io::TestExternalities) -> Result<Self, ProofError> {
        ext.commit_all().map_err(ProofError::OverlayCommit)?;
        let (mut nodes, root) = ext.into_raw_snapshot();
        nodes.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        let checkpoint = Self {
            version: 2,
            identity: Identity::pinned(),
            root,
            nodes,
        };
        checkpoint.validate_nodes()?;
        Ok(checkpoint)
    }
    /// Digest detects accidental corruption; this is not an authentication format.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ProofError> {
        if self.encoded_size() > MAX_CHECKPOINT - 32 {
            return Err(ProofError::Limit);
        }
        let mut bytes = self.encode();
        bytes.extend(sha2_256(&bytes));
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, ProofError> {
        Self::decode_for(bytes, Identity::pinned())
    }
    pub fn decode_for(bytes: &[u8], identity: Identity) -> Result<Self, ProofError> {
        if bytes.len() < 32 || bytes.len() > MAX_CHECKPOINT {
            return Err(ProofError::Limit);
        }
        let (payload, digest) = bytes.split_at(bytes.len() - 32);
        if sha2_256(payload).as_slice() != digest {
            return Err(ProofError::InvalidTrieNode);
        }
        let mut input = payload;
        let checkpoint = Self::decode_with_mem_limit(&mut input, MAX_CHECKPOINT * 2)
            .map_err(ProofError::Decode)?;
        if !input.is_empty() {
            return Err(ProofError::InvalidTrieNode);
        }
        if checkpoint.version != 2 {
            return Err(ProofError::CheckpointVersion(checkpoint.version));
        }
        if checkpoint.identity != identity {
            return Err(ProofError::Identity);
        }
        checkpoint.validate_nodes()?;
        checkpoint.clone().restore()?;
        Ok(checkpoint)
    }
    pub fn root(&self) -> H256 {
        self.root
    }
    fn validate_nodes(&self) -> Result<(), ProofError> {
        if self.nodes.len() > 100_000 || self.encoded_size() > MAX_CHECKPOINT - 32 {
            return Err(ProofError::Limit);
        }
        // raw_iter treats the zero root as an empty trie. Require an actual
        // top-level root node (except for the SDK's canonical empty root).
        let empty = BlakeTwo256::trie_root(Vec::new(), StateVersion::V1);
        if self.root != empty
            && !self
                .nodes
                .iter()
                .any(|(key, _)| key.as_slice() == self.root.as_bytes())
        {
            return Err(ProofError::InvalidTrieNode);
        }
        let mut refs = 0_i64;
        let mut reconstruction_bytes = 0_usize;
        let mut previous: Option<&[u8]> = None;
        for (key, (value, count)) in &self.nodes {
            if key.len() < 32
                || key.len() > 1024
                || *count <= 0
                || *count > 1024
                || previous.is_some_and(|p| p >= key.as_slice())
                || key[key.len() - 32..] != blake2_256(value)
            {
                return Err(ProofError::InvalidTrieNode);
            }
            previous = Some(key);
            refs += i64::from(*count);
            // SDK restore clones the value once per reference. Bound total work,
            // not only serialized size and the number of insertion operations.
            let copies = (value.len() + key.len())
                .checked_mul(*count as usize)
                .ok_or(ProofError::Limit)?;
            reconstruction_bytes = reconstruction_bytes
                .checked_add(copies)
                .ok_or(ProofError::Limit)?;
            if reconstruction_bytes > MAX_CHECKPOINT * 2 {
                return Err(ProofError::Limit);
            }
        }
        if refs > 200_000 {
            return Err(ProofError::Limit);
        }
        Ok(())
    }
    pub fn restore(self) -> Result<sp_io::TestExternalities, ProofError> {
        if self.version != 2 {
            return Err(ProofError::CheckpointVersion(self.version));
        }
        if self.identity != Identity::pinned() {
            return Err(ProofError::Identity);
        }
        self.validate_nodes()?;
        let ext =
            sp_io::TestExternalities::from_raw_snapshot(self.nodes, self.root, StateVersion::V1);
        let backend = &ext.backend;
        let mut entries = 0_usize;
        let mut bytes = 0_usize;
        for pair in backend
            .pairs(IterArgs::default())
            .map_err(|_| ProofError::InvalidTrieNode)?
        {
            let (key, value) = pair.map_err(|_| ProofError::InvalidTrieNode)?;
            budget(&mut entries, &mut bytes, key.len() + value.len())?;
            if let Some(child) = key.strip_prefix(DEFAULT_CHILD_STORAGE_KEY_PREFIX) {
                if value.len() != 32 {
                    return Err(ProofError::InvalidTrieNode);
                }
                let mut args = IterArgs::default();
                args.child_info = Some(ChildInfo::new_default(child));
                for pair in backend
                    .pairs(args)
                    .map_err(|_| ProofError::InvalidTrieNode)?
                {
                    let (key, value) = pair.map_err(|_| ProofError::InvalidTrieNode)?;
                    budget(&mut entries, &mut bytes, key.len() + value.len())?;
                }
            }
        }
        Ok(ext)
    }
}
fn budget(entries: &mut usize, bytes: &mut usize, size: usize) -> Result<(), ProofError> {
    *entries += 1;
    *bytes = bytes.checked_add(size).ok_or(ProofError::Limit)?;
    if *entries > 100_000 || *bytes > MAX_CHECKPOINT {
        return Err(ProofError::Limit);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Checkpoint {
        let mut ext = crate::new_externalities(vec![crate::Account::new([1; 32])]).unwrap();
        ext.execute_with(|| sp_io::default_child_storage::set(b"test-child", b"key", b"value"));
        Checkpoint::capture(ext).unwrap()
    }
    #[test]
    fn rejects_corruption_identity_versions_and_missing_children() {
        let checkpoint = fixture();
        let bytes = checkpoint.to_bytes().unwrap();
        assert!(Checkpoint::decode(&bytes).is_ok());
        let mut bad = bytes.clone();
        bad[20] ^= 1;
        assert!(Checkpoint::decode(&bad).is_err());
        let mut identity = Identity::pinned();
        identity.runtime[0] ^= 1;
        assert!(matches!(
            Checkpoint::decode_for(&bytes, identity),
            Err(ProofError::Identity)
        ));
        // A probe-enabled unit-test checkpoint cannot open in a normal build.
        let mut application_identity = Identity::pinned();
        application_identity.bundles = sha2_256(include_bytes!("../fixtures/manifest.json"));
        assert!(matches!(
            Checkpoint::decode_for(&bytes, application_identity),
            Err(ProofError::Identity)
        ));
        let mut bad = checkpoint.clone();
        bad.version = 1;
        assert!(matches!(
            Checkpoint::decode(&bad.to_bytes().unwrap()),
            Err(ProofError::CheckpointVersion(1))
        ));
        let mut zero = checkpoint.clone();
        zero.root = H256::zero();
        assert!(matches!(
            Checkpoint::decode(&zero.to_bytes().unwrap()),
            Err(ProofError::InvalidTrieNode)
        ));
        assert!(
            Checkpoint::capture(sp_io::TestExternalities::new_empty())
                .unwrap()
                .restore()
                .is_ok()
        );
        let mut bad = checkpoint.clone();
        bad.root = H256::repeat_byte(3);
        assert!(Checkpoint::decode(&bad.to_bytes().unwrap()).is_err());
        let mut bad = checkpoint;
        bad.nodes.retain(|(key, _)| key.len() == 32);
        assert!(Checkpoint::decode(&bad.to_bytes().unwrap()).is_err());
    }
    #[test]
    fn bounds_refcounts_duplicates_and_declared_allocations() {
        let mut bad = fixture();
        bad.nodes[0].1.1 = i32::MAX;
        assert!(Checkpoint::decode(&bad.to_bytes().unwrap()).is_err());
        let mut bad = fixture();
        bad.nodes.push(bad.nodes[0].clone());
        assert!(Checkpoint::decode(&bad.to_bytes().unwrap()).is_err());
        let mut amplified = fixture();
        let value = vec![7; 256 * 1024];
        amplified
            .nodes
            .push((blake2_256(&value).to_vec(), (value, 1024)));
        amplified.nodes.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        assert!(matches!(
            Checkpoint::decode(&amplified.to_bytes().unwrap()),
            Err(ProofError::Limit)
        ));
        let checkpoint = fixture();
        let mut malicious = checkpoint.encode();
        malicious.truncate(4 + 64 + 32);
        malicious.extend(parity_scale_codec::Compact(u32::MAX).encode());
        malicious.extend(sha2_256(&malicious));
        assert!(Checkpoint::decode(&malicious).is_err());
    }
}
