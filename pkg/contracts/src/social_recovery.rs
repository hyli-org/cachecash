use crate::Client;
use crate::error::Result;
use ethereum_types::U64;
use testutil::eth::EthNode;
use web3::{
    contract::{
        Contract,
        tokens::{Tokenizable, TokenizableItem},
    },
    signing::{Key, SecretKey, SecretKeyRef},
    transports::Http,
    types::{Address, H256, U256},
};

/// Represents a guardian entry in the social recovery system
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuardianEntry {
    pub cid_hash: H256,
    pub guardian_value: Vec<u8>,
}

impl Tokenizable for GuardianEntry {
    fn from_token(token: web3::ethabi::Token) -> Result<Self, web3::contract::Error>
    where
        Self: Sized,
    {
        match token {
            web3::ethabi::Token::Tuple(tokens) => {
                if tokens.len() != 2 {
                    return Err(web3::contract::Error::InvalidOutputType(
                        "expected tuple of length 2".to_string(),
                    ));
                }

                let mut tokens = tokens.into_iter();
                let (cid_hash, guardian_value) = (tokens.next().unwrap(), tokens.next().unwrap());

                let cid_hash = H256::from_token(cid_hash)?;
                let guardian_value = Vec::<u8>::from_token(guardian_value)?;

                Ok(Self {
                    cid_hash,
                    guardian_value,
                })
            }
            _ => Err(web3::contract::Error::InvalidOutputType(
                "expected tuple".to_string(),
            )),
        }
    }

    fn into_token(self) -> web3::ethabi::Token {
        web3::ethabi::Token::Tuple(vec![
            self.cid_hash.into_token(),
            self.guardian_value.into_token(),
        ])
    }
}

impl TokenizableItem for GuardianEntry {}

/// Represents the complete guardian configuration for a user
#[derive(Debug, Clone, Default)]
pub struct GuardianConfig {
    pub threshold: U256,
    pub enabled: bool,
    pub guardian_count: U256,
    pub guardians: Vec<GuardianEntry>,
}

/// Social Recovery contract interface
#[derive(Debug, Clone)]
pub struct SocialRecoveryContract {
    client: Client,
    contract: Contract<Http>,
    signer: SecretKey,
    signer_address: Address,
    address: Address,
    /// The ethereum block height used for all contract calls.
    /// If None, the latest block is used.
    block_height: Option<U64>,
}

impl SocialRecoveryContract {
    pub fn new(
        client: Client,
        contract: Contract<Http>,
        signer: SecretKey,
        address: Address,
    ) -> Self {
        let signer_address = Key::address(&SecretKeyRef::new(&signer));

        Self {
            client,
            contract,
            signer,
            signer_address,
            address,
            block_height: None,
        }
    }

    pub fn at_height(self, height: Option<u64>) -> Self {
        Self {
            block_height: height.map(|x| x.into()),
            ..self
        }
    }

    pub fn address(&self) -> Address {
        self.address
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn load(client: Client, contract_addr: &str, signer: SecretKey) -> Result<Self> {
        let contract_json =
            include_str!("../../../eth/artifacts/contracts/SocialRecovery.sol/SocialRecovery.json");
        let contract = client.load_contract_from_str(contract_addr, contract_json)?;

        Ok(Self::new(client, contract, signer, contract_addr.parse()?))
    }

    pub async fn from_eth_node(eth_node: &EthNode, secret_key: SecretKey) -> Result<Self> {
        // This would need to be updated with the actual deployed address
        let social_recovery_addr = "0x27344afff7948003178db8c22481b2422ff703e0";
        let client = Client::from_eth_node(eth_node);
        Self::load(client, social_recovery_addr, secret_key).await
    }

    /// Add a guardian CID for a user (only callable by contract owner)
    #[tracing::instrument(err, ret, skip(self))]
    pub async fn add_guardian_cid(
        &self,
        user: Address,
        guardian_cid: String,
        guardian_value: String,
    ) -> Result<H256> {
        let call_tx = self
            .client
            .call(
                &self.contract,
                "addGuardianCID",
                (user, guardian_cid, guardian_value),
                &self.signer,
                self.signer_address,
            )
            .await?;

        Ok(call_tx)
    }

    /// Update the threshold for a user's guardian configuration (only callable by contract owner)
    #[tracing::instrument(err, ret, skip(self))]
    pub async fn update_threshold(&self, user: Address, new_threshold: U256) -> Result<H256> {
        let call_tx = self
            .client
            .call(
                &self.contract,
                "updateThreshold",
                (user, new_threshold),
                &self.signer,
                self.signer_address,
            )
            .await?;

        Ok(call_tx)
    }

    /// Get the guardian configuration for a user
    #[tracing::instrument(err, ret, skip(self))]
    pub async fn get_guardian_config(&self, user: Address) -> Result<GuardianConfig> {
        let (threshold, enabled, guardian_count, guardians): (
            U256,
            bool,
            U256,
            Vec<GuardianEntry>,
        ) = self
            .client
            .query(
                &self.contract,
                "getGuardianConfig",
                (user,),
                None,
                Default::default(),
                self.block_height.map(|x| x.into()),
            )
            .await?;

        Ok(GuardianConfig {
            threshold,
            enabled,
            guardian_count,
            guardians,
        })
    }

    /// Get the basic guardian configuration (threshold and enabled status) for a user
    #[tracing::instrument(err, ret, skip(self))]
    pub async fn get_basic_guardian_config(&self, user: Address) -> Result<(U256, bool)> {
        let (threshold, enabled): (U256, bool) = self
            .client
            .query(
                &self.contract,
                "guardianConfigs",
                (user,),
                None,
                Default::default(),
                self.block_height.map(|x| x.into()),
            )
            .await?;

        Ok((threshold, enabled))
    }

    /// Get the contract owner address
    #[tracing::instrument(err, ret, skip(self))]
    pub async fn owner(&self) -> Result<Address> {
        let owner: Address = self
            .client
            .query(
                &self.contract,
                "owner",
                (),
                None,
                Default::default(),
                self.block_height.map(|x| x.into()),
            )
            .await?;

        Ok(owner)
    }

    /// Transfer ownership of the contract (only callable by current owner)
    #[tracing::instrument(err, ret, skip(self))]
    pub async fn transfer_ownership(&self, new_owner: Address) -> Result<H256> {
        let call_tx = self
            .client
            .call(
                &self.contract,
                "transferOwnership",
                (new_owner,),
                &self.signer,
                self.signer_address,
            )
            .await?;

        Ok(call_tx)
    }

    /// Renounce ownership of the contract (only callable by current owner)
    #[tracing::instrument(err, ret, skip(self))]
    pub async fn renounce_ownership(&self) -> Result<H256> {
        let call_tx = self
            .client
            .call(
                &self.contract,
                "renounceOwnership",
                (),
                &self.signer,
                self.signer_address,
            )
            .await?;

        Ok(call_tx)
    }
}

impl Default for SocialRecoveryContract {
    fn default() -> Self {
        let client = Client::new("http://localhost:8545", None);
        let signer = SecretKey::from_slice(&[0u8; 32]).expect("valid secret key");
        let contract_json = r#"{"abi": []}"#;
        let contract = client
            .load_contract_from_str("0x0000000000000000000000000000000000000000", contract_json)
            .expect("valid contract");
        let address = "0x0000000000000000000000000000000000000000"
            .parse()
            .expect("valid address");

        Self::new(client, contract, signer, address)
    }
}
