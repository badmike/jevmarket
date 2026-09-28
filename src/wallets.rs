//! The two Polymarket wallets a key owns, and moving pUSD between them through the relayer.
//!
//! - The deposit wallet places the bot's orders (see [`crate::deposit_wallet`]).
//! - The proxy wallet is the polymarket.com account of an email or social login.
//!
//! Transfers only ever go between the wallets derived from the key, whatever the config says,
//! so an edited setting cannot redirect money.

use alloy::network::{Ethereum, Network, TransactionBuilder as _};
use alloy::primitives::{Bytes, U256, address, hex, keccak256};
use alloy::providers::{Provider as _, ProviderBuilder};
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use alloy::sol_types::SolCall as _;
use anyhow::{Context as _, Result, bail};
use polymarket_client_sdk_v2::types::Address;
use polymarket_client_sdk_v2::{POLYGON, contract_config, derive_proxy_wallet, wallet_contract_config};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::Settings;
use crate::deposit_wallet;
use crate::executor::signer;
use crate::relayer::Relayer;

/// Runs proxy wallet transactions for the relayer.
const RELAY_HUB: Address = address!("0xD216153c06E857cD7f72665E0aF1d7D82172F494");
/// When gas estimation fails. Must fit the relay hub's budget of about 650k.
const DEFAULT_GAS_LIMIT: u64 = 500_000;

sol! {
    struct ProxyCall {
        uint8 typeCode;
        address to;
        uint256 value;
        bytes data;
    }

    function proxy(ProxyCall[] calls);

    function transfer(address to, uint256 value) external returns (bool);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Wallet {
    Deposit,
    Proxy,
}

impl Wallet {
    pub fn other(self) -> Self {
        match self {
            Self::Deposit => Self::Proxy,
            Self::Proxy => Self::Deposit,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Deposit => "deposit wallet",
            Self::Proxy => "polymarket.com wallet",
        }
    }
}

/// Both wallets with their pUSD, read on-chain.
#[derive(Debug, Clone, Serialize)]
pub struct Wallets {
    pub signer: Address,
    pub deposit: WalletState,
    pub proxy: WalletState,
    /// Config entries that disagree with the wallets derived from the key.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletState {
    pub address: Address,
    pub balance_usd: f64,
    #[serde(skip)]
    pub balance: U256,
}

impl Wallets {
    pub fn get(&self, w: Wallet) -> &WalletState {
        match w {
            Wallet::Deposit => &self.deposit,
            Wallet::Proxy => &self.proxy,
        }
    }
}

/// What [`transfer`] moved.
#[derive(Debug, Clone, Serialize)]
pub struct Transfer {
    pub from: Wallet,
    pub to: Wallet,
    pub amount_usd: f64,
    pub tx_hash: String,
}

pub async fn load(s: &Settings) -> Result<Wallets> {
    let signer = signer(s)?.address();
    let provider = provider(s)?;
    let deposit = deposit_wallet::address_of(signer, &s.polygon_rpc_url).await?;
    let proxy = derive_proxy_wallet(signer, POLYGON).context("no proxy wallet factory on Polygon")?;
    let mut warnings = Vec::new();
    for (key, configured, derived) in [
        ("polymarket_deposit_wallet", s.polymarket_deposit_wallet, deposit),
        ("polymarket_proxy_wallet", s.polymarket_proxy_wallet, proxy),
    ] {
        if let Some(c) = configured.filter(|c| *c != derived) {
            warnings.push(format!("{key} {c} is not the wallet of this key ({derived}); transfers use {derived}"));
        }
    }
    let state = async |address| -> Result<WalletState> {
        let balance = deposit_wallet::pusd_balance(&provider, address).await?;
        Ok(WalletState { address, balance_usd: to_usd(balance), balance })
    };
    Ok(Wallets { signer, deposit: state(deposit).await?, proxy: state(proxy).await?, warnings })
}

/// Move `amount_usd` of pUSD (all of it when `None`) from `from` to the key's other wallet.
pub async fn transfer(s: &Settings, from: Wallet, amount_usd: Option<f64>) -> Result<Transfer> {
    let key = signer(s)?;
    let wallets = load(s).await?;
    let (source, target) = (wallets.get(from), wallets.get(from.other()));
    let amount = match amount_usd {
        None => source.balance,
        Some(usd) if usd.is_finite() && usd > 0.0 => U256::from((usd * 1e6).round() as u64),
        Some(usd) => bail!("amount ${usd} must be positive"),
    };
    if amount.is_zero() {
        bail!("the {} holds no pUSD", from.label());
    }
    if amount > source.balance {
        bail!("the {} holds ${:.2}, less than ${:.2}", from.label(), source.balance_usd, to_usd(amount));
    }
    let relayer = Relayer::new(s)?;
    tracing::info!("moving ${:.2} pUSD from the {} to the {}", to_usd(amount), from.label(), from.other().label());
    let tx_hash = match from {
        Wallet::Deposit => deposit_wallet::send_pusd(&relayer, &key, source.address, target.address, amount).await?,
        Wallet::Proxy => proxy_send_pusd(&relayer, &key, s, target.address, amount).await?,
    };
    Ok(Transfer { from, to: from.other(), amount_usd: to_usd(amount), tx_hash })
}

/// Send pUSD from the key's proxy wallet: the proxy factory runs the call for its owner, relayed
/// without gas.
async fn proxy_send_pusd(
    relayer: &Relayer,
    key: &PrivateKeySigner,
    s: &Settings,
    to: Address,
    amount: U256,
) -> Result<String> {
    let owner = key.address();
    let factory = wallet_contract_config(POLYGON).and_then(|c| c.proxy_factory).context("no proxy factory")?;
    let pusd = contract_config(POLYGON, false).context("no Polygon contract config")?.collateral;
    let data = proxy_data(pusd, transferCall { to, value: amount }.abi_encode());
    let (relay, nonce) = relayer.relay_payload(owner).await?;
    let estimate =
        <Ethereum as Network>::TransactionRequest::default().with_from(owner).with_to(factory).with_input(data.clone());
    let gas_limit = provider(s)?.estimate_gas(estimate).await.unwrap_or_else(|e| {
        tracing::warn!("gas estimate failed, using {DEFAULT_GAS_LIMIT}: {e}");
        DEFAULT_GAS_LIMIT
    });
    let signature = proxy_signature(key, factory, &data, gas_limit, nonce, relay).await?;
    relayer
        .run(&json!({
            "type": "PROXY",
            "from": owner,
            "to": factory,
            "proxyWallet": derive_proxy_wallet(owner, POLYGON),
            "data": data,
            "signature": signature,
            "signatureParams": {
                "gasPrice": "0",
                "gasLimit": gas_limit.to_string(),
                "relayerFee": "0",
                "relayHub": RELAY_HUB,
                "relay": relay,
            },
            "nonce": nonce.to_string(),
            "metadata": "",
        }))
        .await
}

/// `proxy([(CALL, target, 0, data)])` on the proxy factory.
fn proxy_data(target: Address, call: Vec<u8>) -> Bytes {
    let call = ProxyCall { typeCode: 1, to: target, value: U256::ZERO, data: call.into() };
    proxyCall { calls: vec![call] }.abi_encode().into()
}

/// EIP-191 signature over the relay hub's `rlx:` hash of the transaction.
async fn proxy_signature(
    key: &PrivateKeySigner,
    factory: Address,
    data: &[u8],
    gas_limit: u64,
    nonce: U256,
    relay: Address,
) -> Result<String> {
    let word = |n: U256| n.to_be_bytes::<32>();
    let mut message = b"rlx:".to_vec();
    message.extend_from_slice(key.address().as_slice());
    message.extend_from_slice(factory.as_slice());
    message.extend_from_slice(data);
    message.extend_from_slice(&word(U256::ZERO)); // relayer fee
    message.extend_from_slice(&word(U256::ZERO)); // gas price
    message.extend_from_slice(&word(U256::from(gas_limit)));
    message.extend_from_slice(&word(nonce));
    message.extend_from_slice(RELAY_HUB.as_slice());
    message.extend_from_slice(relay.as_slice());
    let signature = key.sign_message(keccak256(message).as_slice()).await?;
    Ok(format!("0x{}", hex::encode(signature.as_bytes())))
}

fn provider(s: &Settings) -> Result<impl alloy::providers::Provider> {
    Ok(ProviderBuilder::new().connect_http(s.polygon_rpc_url.parse().context("invalid polygon_rpc_url")?))
}

fn to_usd(base_units: U256) -> f64 {
    u64::try_from(base_units).map_or(f64::MAX, |n| n as f64 / 1e6)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::*;

    #[tokio::test]
    async fn proxy_signature_matches_the_reference_client() {
        // py-builder-relayer-client's test_create_proxy_signature, with its public test key.
        let key = PrivateKeySigner::from_str("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
            .unwrap()
            .with_chain_id(Some(137));
        let approve = hex::decode(
            "095ea7b30000000000000000000000004d97dcd97ec945f40cf65f87097ace5ea0476045\
             ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        )
        .unwrap();
        let data = proxy_data(address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"), approve);
        let factory = address!("0xaB45c5A4B0c941a2F231C04C3f49182e1A254052");
        let relay = address!("0xae700edfd9ab986395f3999fe11177b9903a52f1");
        let sig = proxy_signature(&key, factory, &data, 85_338, U256::ZERO, relay).await.unwrap();
        assert_eq!(
            sig,
            "0x4c18e2d2294a00d686714aff8e7936ab657cb4655dfccb2b556efadcb7e835f8\
             00dc2fecec69c501e29bb36ecb54b4da6b7c410c4dc740a33af2afde2b77297e1b"
        );
    }

    #[test]
    fn wallets_pair_up() {
        assert_eq!(Wallet::Deposit.other(), Wallet::Proxy);
        assert_eq!(to_usd(U256::from(59_490_000)), 59.49);
    }
}
