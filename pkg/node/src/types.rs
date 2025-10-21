pub use primitives::block_height::BlockHeight;

use std::fmt::Debug;

use borsh::{BorshDeserialize, BorshSerialize};
use rand_derive2::RandGen;
use serde::{Deserialize, Serialize};

microtype::microtype! {
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize, RandGen, Serialize, Deserialize)]
    pub u64 {
        #[derive(Debug)]
        SnapshotId,
    }
}
