#![deny(clippy::disallowed_methods)]

mod across;
mod client;
mod constants;
mod eip7702;
mod error;
mod rollup;
mod social_recovery;
#[cfg(test)]
mod tests;
mod usdc;
pub mod util;
pub mod wallet;

pub use across::AcrossWithAuthorizationContract;
pub use client::{Client, ConfirmationType};
pub use eip7702::Eip7702Account;
pub use error::{Error, Result};
pub use rollup::RollupContract;
pub use social_recovery::{GuardianConfig, GuardianEntry, SocialRecoveryContract};
pub use usdc::USDCContract;

pub use web3::{
    signing::SecretKey,
    types::{Address, H256, U256},
};

pub use secp256k1::SecretKey as Secp256k1SecretKey;
