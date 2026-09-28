//! Polymarket's relayer: runs wallet transactions on Polygon without gas, authenticated with
//! builder API credentials. Used to deploy and approve the deposit wallet and to move pUSD
//! between the key's wallets.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE};
use hmac::{Hmac, Mac as _};
use polymarket_client_sdk_v2::types::{Address, U256};
use serde_json::Value;
use sha2::Sha256;

use crate::config::Settings;

const RELAYER: &str = "https://relayer-v2.polymarket.com";
/// Transactions are awaited this many times, two seconds apart.
const POLLS: u32 = 60;

pub struct Relayer {
    http: reqwest::Client,
    key: String,
    secret: Vec<u8>,
    passphrase: String,
}

impl Relayer {
    pub fn new(s: &Settings) -> Result<Self> {
        let (key, secret, passphrase) =
            (&s.polymarket_builder_api_key, &s.polymarket_builder_secret, &s.polymarket_builder_passphrase);
        if key.is_empty() || secret.is_empty() || passphrase.is_empty() {
            bail!(
                "Polymarket's relayer needs builder API credentials. Create them at \
                 https://polymarket.com/settings?tab=builder, then run\n  \
                 jevmarket config set polymarket_builder_api_key <key>\n  \
                 jevmarket config set polymarket_builder_secret <secret>\n  \
                 jevmarket config set polymarket_builder_passphrase <passphrase>\n\
                 or export POLYMARKET_BUILDER_API_KEY, POLYMARKET_BUILDER_SECRET and POLYMARKET_BUILDER_PASSPHRASE"
            );
        }
        let secret =
            URL_SAFE.decode(secret).or_else(|_| STANDARD.decode(secret)).context("builder secret is not base64")?;
        let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
        Ok(Self { http, key: key.clone(), secret, passphrase: passphrase.clone() })
    }

    /// Submit a transaction and wait until it is mined; returns the transaction hash.
    pub async fn run(&self, body: &Value) -> Result<String> {
        let id = self.submit(body).await?;
        self.wait(&id).await
    }

    /// Submit a transaction; returns its relayer id.
    async fn submit(&self, body: &Value) -> Result<String> {
        let body = body.to_string();
        let timestamp = unix_now().to_string();
        let response = self
            .http
            .post(format!("{RELAYER}/submit"))
            .header("content-type", "application/json")
            .header("POLY_BUILDER_API_KEY", &self.key)
            .header("POLY_BUILDER_PASSPHRASE", &self.passphrase)
            .header("POLY_BUILDER_TIMESTAMP", &timestamp)
            .header("POLY_BUILDER_SIGNATURE", builder_signature(&self.secret, &timestamp, "POST", "/submit", &body))
            .body(body)
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            bail!("relayer refused the transaction ({status}): {text}");
        }
        let reply: Value = serde_json::from_str(&text).with_context(|| format!("relayer reply: {text}"))?;
        reply["transactionID"].as_str().map(str::to_owned).with_context(|| format!("no transactionID in {text}"))
    }

    /// The next nonce for `owner`'s transactions of `kind` (`WALLET` for deposit wallet batches).
    pub async fn nonce(&self, owner: Address, kind: &str) -> Result<U256> {
        let reply = self.get(&format!("/nonce?address={owner}&type={kind}")).await?;
        parse_u256(&reply["nonce"]).with_context(|| format!("relayer nonce: {reply}"))
    }

    /// `(relay address, nonce)` for a proxy wallet transaction by `owner`.
    pub async fn relay_payload(&self, owner: Address) -> Result<(Address, U256)> {
        let reply = self.get(&format!("/relay-payload?address={owner}&type=PROXY")).await?;
        let relay = reply["address"].as_str().and_then(|a| a.parse().ok());
        let nonce = parse_u256(&reply["nonce"]);
        relay.zip(nonce).with_context(|| format!("relay payload: {reply}"))
    }

    async fn get(&self, path: &str) -> Result<Value> {
        Ok(self.http.get(format!("{RELAYER}{path}")).send().await?.error_for_status()?.json().await?)
    }

    async fn wait(&self, id: &str) -> Result<String> {
        for _ in 0..POLLS {
            let reply = self.get(&format!("/transaction?id={id}")).await?;
            let tx = &reply[0];
            let hash = tx["transactionHash"].as_str().unwrap_or_default().to_owned();
            match tx["state"].as_str() {
                Some("STATE_MINED" | "STATE_CONFIRMED") => {
                    tracing::info!("relayer transaction {id} mined: {hash}");
                    return Ok(hash);
                }
                Some(state @ ("STATE_FAILED" | "STATE_INVALID")) => {
                    bail!("relayer transaction {id} ended in {state} (hash {hash})")
                }
                _ => tokio::time::sleep(Duration::from_secs(2)).await,
            }
        }
        bail!("relayer transaction {id} not mined after {}s; check it again later", POLLS * 2)
    }
}

/// The relayer sends numbers as strings, sometimes as JSON numbers.
fn parse_u256(v: &Value) -> Option<U256> {
    v.as_str().map(str::to_owned).or_else(|| v.as_u64().map(|n| n.to_string()))?.parse().ok()
}

/// `POLY_BUILDER_SIGNATURE`: HMAC-SHA256 of `timestamp + method + path + body`, URL-safe base64.
fn builder_signature(secret: &[u8], timestamp: &str, method: &str, path: &str, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC takes keys of any length");
    mac.update(format!("{timestamp}{method}{path}{body}").as_bytes());
    URL_SAFE.encode(mac.finalize().into_bytes())
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_signature_matches_the_reference_sdk() {
        // Reference value from @polymarket/builder-signing-sdk's buildHmacSignature.
        let secret = URL_SAFE.decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap();
        let sig = builder_signature(&secret, "1000000", "POST", "/submit", r#"{"a":1}"#);
        assert_eq!(sig, "QlkyEqlnsVAb-t8IQFJ_4qjz5NV02H7wLl8VJ7Lvaag=");
    }
}
