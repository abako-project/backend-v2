//! Full trie snapshot, including contracts' child tries. Not a provider format yet.
use crate::ProofError;
use parity_scale_codec::{Decode, DecodeAll, Encode};
use sp_core::H256;
use sp_runtime::StateVersion;

type RawNodes = Vec<(Vec<u8>, (Vec<u8>, i32))>;

#[derive(Encode, Decode)]
pub struct Checkpoint {
    version: u32,
    root: H256,
    nodes: RawNodes,
}
impl Checkpoint {
    pub fn capture(mut ext: sp_io::TestExternalities) -> Result<Self, ProofError> {
        ext.commit_all()
            .map_err(|e| ProofError::OverlayCommit(format!("{e:?}")))?;
        let (nodes, root) = ext.into_raw_snapshot();
        Ok(Self {
            version: 1,
            root,
            nodes,
        })
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, ProofError> {
        let checkpoint = Self::decode_all(&mut &*bytes).map_err(ProofError::Decode)?;
        if checkpoint.version != 1 {
            return Err(ProofError::CheckpointVersion(checkpoint.version));
        }
        if checkpoint
            .nodes
            .iter()
            .any(|(key, (_, count))| key.len() < 32 || *count <= 0 || *count > 1_000_000)
        {
            return Err(ProofError::InvalidTrieNode);
        }
        Ok(checkpoint)
    }
    pub fn restore(self) -> sp_io::TestExternalities {
        sp_io::TestExternalities::from_raw_snapshot(self.nodes, self.root, StateVersion::V1)
    }
}
