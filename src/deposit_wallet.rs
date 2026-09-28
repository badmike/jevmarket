//! Deposit wallets: the smart wallet Polymarket requires new API traders to place orders from.
//! Orders from a raw EOA or a polymarket.com proxy or Safe wallet are refused with "maker
//! address not allowed, please use the deposit wallet flow".
//!
//! A deposit wallet is a per-signer contract at an address derived from the signer. Polymarket's
//! relayer deploys it and runs its calls without gas, authenticated with builder API
//! credentials. Orders are signed by the key and checked against the wallet through ERC-1271
//! (`SignatureType::Poly1271`). See <https://docs.polymarket.com/trading/deposit-wallets>.

use alloy::primitives::{B256, Bytes, address, hex};
use alloy::providers::{Provider as _, ProviderBuilder};
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use alloy::sol_types::{SolCall as _, SolStruct as _, eip712_domain};
use anyhow::{Context as _, Result};
use polymarket_client_sdk_v2::bridge::types::DepositRequest;
use polymarket_client_sdk_v2::types::{Address, U256};
use polymarket_client_sdk_v2::{POLYGON, bridge, contract_config};
use serde_json::{Value, json};

use crate::config::Settings;
use crate::relayer::{Relayer, unix_now};

const FACTORY: Address = address!("0x00000000000Fb5C9ADea0298D729A0CB3823Cc07");

sol! {
    #[sol(rpc)]
    interface IDepositWalletFactory {
        function predictWalletAddress(bytes32 id) external view returns (address);
        function predictLegacyWalletAddress(bytes32 id) external view returns (address);
    }

    #[sol(rpc)]
    interface IERC20 {
        function approve(address spender, uint256 value) external returns (bool);
        function transfer(address to, uint256 value) external returns (bool);
        function allowance(address owner, address spender) external view returns (uint256);
        function balanceOf(address owner) external view returns (uint256);
    }

    #[sol(rpc)]
    interface IERC1155 {
        function setApprovalForAll(address operator, bool approved) external;
        function isApprovedForAll(address account, address operator) external view returns (bool);
    }

    /// One call the wallet makes, EIP-712 signed as part of a [`Batch`].
    struct Call {
        address target;
        uint256 value;
        bytes data;
    }

    struct Batch {
        address wallet;
        uint256 nonce;
        uint256 deadline;
        Call[] calls;
    }
}

/// What [`ensure`] found and did.
#[derive(Debug)]
pub struct Ready {
    pub wallet: Address,
    pub deployed_now: bool,
    /// Approvals granted in this run, 0 when all were in place.
    pub approved_now: usize,
}

/// The signer's deposit wallet, deployed and approved for the exchange contracts. Only what is
/// missing is sent to the relayer, so running it again is safe and free.
pub async fn ensure(signer: &PrivateKeySigner, s: &Settings) -> Result<Ready> {
    let rpc = s.polygon_rpc_url.parse().context("invalid polygon_rpc_url")?;
    let provider = ProviderBuilder::new().connect_http(rpc);
    let owner = signer.address();
    let wallet = predict(&provider, owner).await?;

    let relayer = || Relayer::new(s).with_context(|| format!("setting up deposit wallet {wallet}"));
    let deployed_now = if provider.get_code_at(wallet).await?.is_empty() {
        tracing::info!("deploying deposit wallet {wallet}");
        let create = json!({"type": "WALLET-CREATE", "from": owner, "to": FACTORY});
        relayer()?.run(&create).await.context("deploying the deposit wallet")?;
        true
    } else {
        false
    };

    let calls = missing_approvals(&provider, wallet).await?;
    let approved_now = calls.len();
    if !calls.is_empty() {
        tracing::info!("approving {approved_now} exchange allowances for {wallet}");
        execute(&relayer()?, signer, wallet, calls).await.context("approving the exchange")?;
    }
    Ok(Ready { wallet, deployed_now, approved_now })
}

/// Send `amount` pUSD base units (6 decimals) from the deposit wallet to `to`; returns the
/// transaction hash.
pub async fn send_pusd(
    relayer: &Relayer,
    signer: &PrivateKeySigner,
    wallet: Address,
    to: Address,
    amount: U256,
) -> Result<String> {
    let pusd = contract_config(POLYGON, false).context("no Polygon contract config")?.collateral;
    let data = IERC20::transferCall { to, value: amount }.abi_encode();
    execute(relayer, signer, wallet, vec![Call { target: pusd, value: U256::ZERO, data: Bytes::from(data) }]).await
}

/// pUSD base units `owner` holds.
pub async fn pusd_balance(provider: &impl alloy::providers::Provider, owner: Address) -> Result<U256> {
    let pusd = contract_config(POLYGON, false).context("no Polygon contract config")?.collateral;
    Ok(IERC20::new(pusd, provider).balanceOf(owner).call().await?)
}

/// Run `calls` from the deposit wallet as one batch signed by its owner.
async fn execute(relayer: &Relayer, signer: &PrivateKeySigner, wallet: Address, calls: Vec<Call>) -> Result<String> {
    let owner = signer.address();
    let nonce = relayer.nonce(owner, "WALLET").await?;
    let deadline = unix_now() + 300;
    let batch = Batch { wallet, nonce, deadline: U256::from(deadline), calls };
    let domain = eip712_domain! {
        name: "DepositWallet",
        version: "1",
        chain_id: POLYGON,
        verifying_contract: wallet,
    };
    let signature = signer.sign_hash(&batch.eip712_signing_hash(&domain)).await?;
    let calls: Vec<Value> =
        batch.calls.iter().map(|c| json!({"target": c.target, "value": c.value.to_string(), "data": c.data})).collect();
    relayer
        .run(&json!({
            "type": "WALLET",
            "from": owner,
            "to": FACTORY,
            "nonce": nonce.to_string(),
            "signature": format!("0x{}", hex::encode(signature.as_bytes())),
            "depositWalletParams": {"depositWallet": wallet, "deadline": deadline.to_string(), "calls": calls},
        }))
        .await
}

/// The address the bridge credits to `wallet` as pUSD: send USDC on Polygon (or any chain and
/// asset the bridge supports) there to fund it.
pub async fn funding_address(wallet: Address) -> Result<Address> {
    let request = DepositRequest::builder().address(wallet).build();
    Ok(bridge::Client::default()
        .deposit(&request)
        .await
        .context("asking the bridge for a deposit address")?
        .address
        .evm)
}

/// The deposit wallet `owner` signs for.
pub async fn address_of(owner: Address, rpc_url: &str) -> Result<Address> {
    let provider = ProviderBuilder::new().connect_http(rpc_url.parse().context("invalid polygon_rpc_url")?);
    predict(&provider, owner).await
}

/// Wallets deployed before the factory moved to beacon proxies keep their legacy address.
async fn predict(provider: &impl alloy::providers::Provider, owner: Address) -> Result<Address> {
    let factory = IDepositWalletFactory::new(FACTORY, provider);
    let id: B256 = owner.into_word();
    let legacy = factory.predictLegacyWalletAddress(id).call().await.context("deposit wallet factory")?;
    if !provider.get_code_at(legacy).await?.is_empty() {
        return Ok(legacy);
    }
    Ok(factory.predictWalletAddress(id).call().await?)
}

/// pUSD allowances and outcome-token approvals the wallet lacks for the V2 exchanges, where
/// orders settle, and the neg-risk adapter, which neg-risk orders also draw on. Exchange V3 is
/// left out: the relayer's allow list refuses it.
async fn missing_approvals(provider: &impl alloy::providers::Provider, wallet: Address) -> Result<Vec<Call>> {
    let (standard, neg_risk) = (
        contract_config(POLYGON, false).context("no Polygon contract config")?,
        contract_config(POLYGON, true).context("no Polygon neg-risk contract config")?,
    );
    let spenders: Vec<Address> =
        [standard.exchange_v2, neg_risk.exchange_v2, neg_risk.neg_risk_adapter].into_iter().flatten().collect();
    let collateral = IERC20::new(standard.collateral, provider);
    let ctf = IERC1155::new(standard.conditional_tokens, provider);
    let mut calls = Vec::new();
    for spender in spenders {
        if collateral.allowance(wallet, spender).call().await? < U256::MAX >> 1 {
            let data = IERC20::approveCall { spender, value: U256::MAX }.abi_encode();
            calls.push(Call { target: standard.collateral, value: U256::ZERO, data: Bytes::from(data) });
        }
        if !ctf.isApprovedForAll(wallet, spender).call().await? {
            let data = IERC1155::setApprovalForAllCall { operator: spender, approved: true }.abi_encode();
            calls.push(Call { target: standard.conditional_tokens, value: U256::ZERO, data: Bytes::from(data) });
        }
    }
    Ok(calls)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference value from viem's `hashTypedData`.
    #[test]
    fn batch_hash_matches_viem() {
        let wallet = address!("0x1111111111111111111111111111111111111111");
        let call = Call {
            target: address!("0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB"),
            value: U256::ZERO,
            data: Bytes::from_static(&[0xde, 0xad, 0xbe, 0xef]),
        };
        let batch = Batch { wallet, nonce: U256::from(3), deadline: U256::from(1_790_000_000), calls: vec![call] };
        let domain = eip712_domain! { name: "DepositWallet", version: "1", chain_id: 137, verifying_contract: wallet, };
        assert_eq!(
            batch.eip712_signing_hash(&domain).to_string(),
            "0x6343443b93a508d9550d636377d18e45c31804d0249ecfef8e8f3651c5310e5c"
        );
    }
}
