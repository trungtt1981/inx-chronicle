// Copyright 2022 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crypto::hashes::{blake2b::Blake2b256, Digest, Output};

const LEAF_HASH_PREFIX: u8 = 0;
const NODE_HASH_PREFIX: u8 = 1;

pub type MerkleHash = Output<Blake2b256>;

/// A Merkle tree hasher that uses the `Blake2b256` hash function.
pub struct MerkleHasher;

impl MerkleHasher {
    pub fn hash(data: &[impl AsRef<[u8]>]) -> MerkleHash {
        match data {
            [] => Self::hash_empty(),
            [leaf] => Self::hash_leaf(leaf),
            _ => {
                let k = largest_power_of_two(data.len());
                let l = Self::hash(&data[..k]);
                let r = Self::hash(&data[k..]);
                Self::hash_node(l, r)
            }
        }
    }

    pub fn hash_empty() -> MerkleHash {
        Blake2b256::digest([])
    }

    pub fn hash_leaf(l: impl AsRef<[u8]>) -> MerkleHash {
        let mut hasher = Blake2b256::default();
        hasher.update([LEAF_HASH_PREFIX]);
        hasher.update(l);
        hasher.finalize()
    }

    pub fn hash_node(l: impl AsRef<[u8]>, r: impl AsRef<[u8]>) -> MerkleHash {
        let mut hasher = Blake2b256::default();
        hasher.update([NODE_HASH_PREFIX]);
        hasher.update(l);
        hasher.update(r);
        hasher.finalize()
    }
}

/// Returns the largest power of 2 less than a given number `n`.
pub(crate) fn largest_power_of_two(n: usize) -> usize {
    debug_assert!(n > 1, "invalid input");
    1 << (bit_length((n - 1) as u32) - 1)
}

const fn bit_length(n: u32) -> u32 {
    32 - n.leading_zeros()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chronicle::model::BlockId;

    use super::*;

    impl MerkleHasher {
        pub fn hash_block_ids(data: &[BlockId]) -> MerkleHash {
            let data = data.iter().map(|id| &id.0[..]).collect::<Vec<_>>();
            Self::hash(&data[..])
        }
    }

    #[test]
    fn test_largest_power_of_two_lte_number() {
        assert_eq!(2u32.pow(0) as usize, largest_power_of_two(2));
        assert_eq!(2u32.pow(1) as usize, largest_power_of_two(3));
        assert_eq!(2u32.pow(1) as usize, largest_power_of_two(4));
        assert_eq!(2u32.pow(31) as usize, largest_power_of_two(u32::MAX as usize));
    }

    #[test]
    fn test_merkle_tree_hasher_empty() {
        let root = MerkleHasher::hash_block_ids(&[]);
        assert_eq!(
            prefix_hex::encode(root.as_slice()),
            "0x0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8"
        )
    }

    #[test]
    fn test_merkle_tree_hasher_single() {
        let root = MerkleHasher::hash_block_ids(&[BlockId::from_str(
            "0x52fdfc072182654f163f5f0f9a621d729566c74d10037c4d7bbb0407d1e2c649",
        )
        .unwrap()]);

        assert_eq!(
            prefix_hex::encode(root.as_slice()),
            "0x3d1399c64ff0ae6a074afa4cd2ce4eab8d5c499c1da6afdd1d84b7447cc00544"
        )
    }

    #[test]
    fn test_merkle_tree_root() {
        let block_ids = [
            "0x52fdfc072182654f163f5f0f9a621d729566c74d10037c4d7bbb0407d1e2c649",
            "0x81855ad8681d0d86d1e91e00167939cb6694d2c422acd208a0072939487f6999",
            "0xeb9d18a44784045d87f3c67cf22746e995af5a25367951baa2ff6cd471c483f1",
            "0x5fb90badb37c5821b6d95526a41a9504680b4e7c8b763a1b1d49d4955c848621",
            "0x6325253fec738dd7a9e28bf921119c160f0702448615bbda08313f6a8eb668d2",
            "0x0bf5059875921e668a5bdf2c7fc4844592d2572bcd0668d2d6c52f5054e2d083",
            "0x6bf84c7174cb7476364cc3dbd968b0f7172ed85794bb358b0c3b525da1786f9f",
        ]
        .iter()
        .map(|hash| BlockId::from_str(hash).unwrap())
        .collect::<Vec<_>>();

        let merkle_root = MerkleHasher::hash_block_ids(&block_ids);

        assert_eq!(
            prefix_hex::encode(merkle_root.as_slice()),
            "0xbf67ce7ba23e8c0951b5abaec4f5524360d2c26d971ff226d3359fa70cdb0beb"
        )
    }
}
