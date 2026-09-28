//! Order execution with hard caps. The only module that can spend money.

use std::collections::HashSet;
use std::str::FromStr as _;
use std::time::Duration;

use alloy::providers::ProviderBuilder;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use anyhow::{Context as _, Result, bail};
use futures::TryStreamExt as _;
use polymarket_client_sdk_v2::auth::Normal;
use polymarket_client_sdk_v2::auth::state::Authenticated;
use polymarket_client_sdk_v2::clob::types::request::{BalanceAllowanceRequest, OrdersRequest};
use polymarket_client_sdk_v2::clob::types::response::{OpenOrderResponse, PostOrderResponse};
use polymarket_client_sdk_v2::clob::types::{OrderType, Side, SignatureType};
use polymarket_client_sdk_v2::data::types::request::PositionsRequest;
use polymarket_client_sdk_v2::data::types::response::Position;
use polymarket_client_sdk_v2::types::{Address, Decimal, U256};
use polymarket_client_sdk_v2::{POLYGON, clob, contract_config, data, derive_proxy_wallet, derive_safe_wallet};
use rust_decimal::prelude::ToPrimitive as _;
use serde_json::json;

use crate::config::Settings;
use crate::markets::Candidate;
use crate::signal::Trade;
use crate::store::{OrderRow, Store};

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function approve(address spender, uint256 value) external returns (bool);
        function allowance(address owner, address spender) external view returns (uint256);
    }

    #[sol(rpc)]
    interface IERC1155 {
        function setApprovalForAll(address operator, bool approved) external;
        function isApprovedForAll(address account, address operator) external view returns (bool);
    }
}

const POSITIONS_PAGE: i32 = 500;

#[derive(Debug, Default, Clone)]
pub struct Exposure {
    pub positions_usd: f64,
    pub open_orders_usd: f64,
    pub condition_ids: HashSet<String>,
}

impl Exposure {
    pub fn total(&self) -> f64 {
        self.positions_usd + self.open_orders_usd
    }
}

#[derive(Debug)]
pub struct Placed {
    pub ok: bool,
    pub order_id: Option<String>,
    pub status: String,
    pub message: Option<String>,
}

struct Trader {
    client: clob::Client<Authenticated<Normal>>,
    signer: PrivateKeySigner,
    kind: SignatureType,
}

/// Either an authenticated trader, or (dry runs without a key) a read-only view of the
/// configured wallet address.
pub struct Executor<'a> {
    s: &'a Settings,
    store: &'a Store,
    dry_run: bool,
    data: data::Client,
    trader: Option<Trader>,
    pub wallet: Option<Address>,
    pub trades_this_run: u32,
    exposure: Option<Exposure>,
}

impl<'a> Executor<'a> {
    pub async fn create(s: &'a Settings, store: &'a Store, dry_run: bool) -> Result<Self> {
        let key = s.polymarket_private_key.trim();
        let mut ex = Self {
            s,
            store,
            dry_run,
            data: data::Client::default(),
            trader: None,
            wallet: s.polymarket_wallet,
            trades_this_run: 0,
            exposure: None,
        };
        if key.trim_start_matches("0x").len() != 64 {
            if !dry_run {
                bail!(
                    "polymarket_private_key must be a 32-byte hex key to trade live \
                     (a 0x…40-hex value is an address, not a key)"
                );
            }
            tracing::info!("no private key: dry run with address-only exposure for {}", ex.wallet_label());
            return Ok(ex);
        }

        let signer =
            PrivateKeySigner::from_str(key).context("invalid polymarket_private_key")?.with_chain_id(Some(POLYGON));
        let eoa = signer.address();
        let funder = s.polymarket_wallet.filter(|w| *w != eoa);
        let kind = wallet_kind(eoa, funder);
        let mut auth = clob::Client::new(&s.clob_host, clob::Config::default())?
            .authentication_builder(&signer)
            .signature_type(kind);
        if let Some(w) = funder {
            auth = auth.funder(w);
        }
        let client = auth.authenticate().await.context("Polymarket authentication failed")?;
        ex.wallet = Some(funder.unwrap_or(eoa));
        ex.trader = Some(Trader { client, signer, kind });
        tracing::info!("wallet {} ({}) dry_run={dry_run}", ex.wallet_label(), ex.wallet_type());
        Ok(ex)
    }

    pub fn wallet_label(&self) -> String {
        self.wallet.map_or_else(|| "<no wallet>".into(), |w| w.to_string())
    }

    pub fn wallet_type(&self) -> &'static str {
        self.trader.as_ref().map_or("address-only", |t| kind_label(t.kind))
    }

    pub fn authenticated(&self) -> bool {
        self.trader.is_some()
    }

    // --- state --------------------------------------------------------------

    pub async fn positions(&self) -> Result<Vec<Position>> {
        let Some(user) = self.wallet else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        loop {
            let request = PositionsRequest::builder()
                .user(user)
                .limit(POSITIONS_PAGE)?
                .offset(i32::try_from(out.len()).unwrap_or(i32::MAX))?
                .build();
            let page = self.data.positions(&request).await.context("loading positions")?;
            let done = page.len() < POSITIONS_PAGE as usize;
            out.extend(page);
            if done {
                return Ok(out);
            }
        }
    }

    pub async fn open_orders(&self) -> Result<Vec<OpenOrderResponse>> {
        let Some(t) = &self.trader else { return Ok(Vec::new()) };
        let stream = t.client.stream_data(|c, cursor| async move { c.orders(&OrdersRequest::default(), cursor).await });
        stream.try_collect().await.context("loading open orders")
    }

    /// Positions plus open buy orders, cached until `refresh`.
    pub async fn exposure(&mut self, refresh: bool) -> Result<&Exposure> {
        if refresh || self.exposure.is_none() {
            let mut ex = Exposure::default();
            for p in self.positions().await? {
                ex.positions_usd += p.current_value.to_f64().unwrap_or_default();
                ex.condition_ids.insert(p.condition_id.to_string());
            }
            for o in self.open_orders().await? {
                if o.side == Side::Buy {
                    ex.open_orders_usd += ((o.original_size - o.size_matched) * o.price).to_f64().unwrap_or_default();
                }
                ex.condition_ids.insert(o.market.to_string());
            }
            self.exposure = Some(ex);
        }
        Ok(self.exposure.get_or_insert_default())
    }

    /// pUSD balance the exchange sees, or `None` without a key.
    pub async fn collateral_balance_usd(&self) -> Option<f64> {
        let t = self.trader.as_ref()?;
        match t.client.balance_allowance(BalanceAllowanceRequest::default()).await {
            // Base units, 6 decimals.
            Ok(ba) => ba.balance.to_f64().map(|b| b / 1e6),
            Err(e) => {
                tracing::warn!("balance lookup failed: {e}");
                None
            }
        }
    }

    // --- guards -------------------------------------------------------------

    /// A refusal reason, or `None` if the trade may go ahead. A manual order overrides the
    /// per-run trade count and the one-order-per-market rule, never the money caps.
    pub async fn check(&mut self, c: &Candidate, t: &Trade, manual: bool) -> Result<Option<String>> {
        let (max_trades, max_usd, max_exposure) =
            (self.s.max_trades_per_run, self.s.max_usd_per_trade, self.s.max_open_exposure_usd);
        if !manual && self.trades_this_run >= max_trades {
            return Ok(Some(format!("max_trades_per_run={max_trades} reached")));
        }
        if t.usd > max_usd * 1.5 {
            return Ok(Some(format!("${:.2} exceeds per-trade cap", t.usd)));
        }
        if !manual && self.store.has_order_for(&c.market.condition_id, false)? {
            return Ok(Some("already ordered on this market (db)".into()));
        }
        let ex = self.exposure(false).await?;
        if !manual && ex.condition_ids.contains(&c.market.condition_id) {
            return Ok(Some("already exposed to this market (chain)".into()));
        }
        if ex.total() + t.usd > max_exposure {
            return Ok(Some(format!("exposure ${:.2} + ${:.2} > cap ${max_exposure:.2}", ex.total(), t.usd)));
        }
        Ok(None)
    }

    // --- action -------------------------------------------------------------

    /// Place a trade the signal proposed, or with `manual`, one a person asked for.
    pub async fn place(&mut self, c: &Candidate, t: &Trade, manual: bool) -> Result<Placed> {
        let slug = c.market.slug.as_str();
        if let Some(refusal) = self.check(c, t, manual).await? {
            tracing::info!("refuse {slug}: {refusal}");
            return Ok(Placed { ok: false, order_id: None, status: "refused".into(), message: Some(refusal) });
        }
        let token_id = t.token_id.to_string();
        let outcome = t.outcome.to_string();
        let row = OrderRow {
            slug,
            condition_id: &c.market.condition_id,
            token_id: &token_id,
            outcome: &outcome,
            price: t.price,
            size: t.size,
            usd: t.usd,
            manual,
            ..OrderRow::default()
        };

        if self.dry_run {
            self.record_fill(c, t);
            self.store.log_order(&OrderRow { status: "dry_run", dry_run: true, ..row })?;
            tracing::info!("DRY RUN: would BUY {outcome} x{:.2} @ {:.3} (${:.2}) on {slug}", t.size, t.price, t.usd);
            return Ok(Placed { ok: true, order_id: None, status: "dry_run".into(), message: None });
        }

        let trader = self.trader.as_ref().expect("live executors are authenticated");
        match post_limit_buy(trader, t.token_id, t.price, t.size).await {
            Ok(resp) if resp.success => {
                let status = resp.status.to_string().to_lowercase();
                self.record_fill(c, t);
                self.store.log_order(&OrderRow {
                    order_id: Some(&resp.order_id),
                    status: &status,
                    response: Some(&response_json(&resp)),
                    ..row
                })?;
                tracing::info!(
                    "PLACED {slug}: BUY {outcome} x{:.2} @ {:.3} (${:.2}) id={} status={status}",
                    t.size,
                    t.price,
                    t.usd,
                    resp.order_id
                );
                Ok(Placed { ok: true, order_id: Some(resp.order_id), status, message: None })
            }
            outcome_err => {
                let (message, response) = match outcome_err {
                    Ok(resp) => {
                        (resp.error_msg.clone().unwrap_or_else(|| resp.status.to_string()), Some(response_json(&resp)))
                    }
                    Err(e) => (format!("{e:#}"), None),
                };
                self.store.log_order(&OrderRow { status: "rejected", response: response.as_ref(), ..row })?;
                tracing::warn!("REJECTED {slug}: {message}");
                Ok(Placed { ok: false, order_id: None, status: "rejected".into(), message: Some(message) })
            }
        }
    }

    fn record_fill(&mut self, c: &Candidate, t: &Trade) {
        self.trades_this_run += 1;
        if let Some(ex) = &mut self.exposure {
            ex.open_orders_usd += t.usd;
            ex.condition_ids.insert(c.market.condition_id.clone());
        }
    }

    /// One-time ERC-20 (pUSD) and ERC-1155 (outcome token) approvals for the exchange contracts.
    /// Only raw EOAs need this; Polymarket manages approvals for its own wallets. The public
    /// Polygon RPC fails intermittently mid-batch; granted approvals are skipped, so retry.
    pub async fn setup_approvals(&self, attempts: u32) -> Result<String> {
        let Some(t) = &self.trader else { bail!("setup needs polymarket_private_key") };
        if t.kind != SignatureType::Eoa {
            return Ok(format!("nothing to do: approvals for a {} are managed by Polymarket", self.wallet_type()));
        }
        let mut last = None;
        for attempt in 1..=attempts {
            match approve_all(&t.signer, &self.s.polygon_rpc_url).await {
                Ok(()) => {
                    // Let the CLOB refresh its cached view of the new allowances.
                    if let Err(e) = t.client.update_balance_allowance(BalanceAllowanceRequest::default()).await {
                        tracing::warn!("balance cache refresh failed: {e}");
                    }
                    return Ok("fully approved".into());
                }
                Err(e) => {
                    tracing::warn!("approvals attempt {attempt}/{attempts}: {e:#}");
                    last = Some(e);
                    tokio::time::sleep(Duration::from_secs(2 * u64::from(attempt))).await;
                }
            }
        }
        Err(last.expect("at least one attempt").context(format!("approvals incomplete after {attempts} attempts")))
    }
}

/// How `eoa` controls `funder`: Polymarket derives Safe and proxy wallets from the signer's
/// address, so anything else is treated as a deposit wallet. No funder means a raw EOA.
pub fn wallet_kind(eoa: Address, funder: Option<Address>) -> SignatureType {
    match funder {
        None => SignatureType::Eoa,
        Some(w) if w == eoa => SignatureType::Eoa,
        Some(w) if Some(w) == derive_safe_wallet(eoa, POLYGON) => SignatureType::GnosisSafe,
        Some(w) if Some(w) == derive_proxy_wallet(eoa, POLYGON) => SignatureType::Proxy,
        Some(_) => SignatureType::Poly1271,
    }
}

pub fn kind_label(kind: SignatureType) -> &'static str {
    match kind {
        SignatureType::Eoa => "EOA",
        SignatureType::Proxy => "proxy",
        SignatureType::GnosisSafe => "safe",
        _ => "deposit wallet",
    }
}

async fn post_limit_buy(t: &Trader, token_id: U256, price: f64, size: f64) -> Result<PostOrderResponse> {
    let order = t
        .client
        .limit_order()
        .token_id(token_id)
        .side(Side::Buy)
        .price(Decimal::try_from(price)?.round_dp(4))
        .size(Decimal::try_from(size)?.round_dp(2))
        .order_type(OrderType::GTC)
        .build()
        .await?;
    let signed = t.client.sign(&t.signer, order).await?;
    Ok(t.client.post_order(signed).await?)
}

fn response_json(r: &PostOrderResponse) -> serde_json::Value {
    json!({
        "order_id": r.order_id,
        "status": r.status.to_string(),
        "success": r.success,
        "error_msg": r.error_msg,
        "making_amount": r.making_amount.to_string(),
        "taking_amount": r.taking_amount.to_string(),
        "trade_ids": r.trade_ids,
    })
}

async fn approve_all(signer: &PrivateKeySigner, rpc_url: &str) -> Result<()> {
    let provider =
        ProviderBuilder::new().wallet(signer.clone()).connect_http(rpc_url.parse().context("invalid polygon_rpc_url")?);
    let owner = signer.address();
    let (standard, neg_risk) = (
        contract_config(POLYGON, false).context("no Polygon contract config")?,
        contract_config(POLYGON, true).context("no Polygon neg-risk contract config")?,
    );
    let collateral = IERC20::new(standard.collateral, &provider);
    let ctf = IERC1155::new(standard.conditional_tokens, &provider);
    let spenders = [
        ("CTF Exchange V2", standard.exchange_v2),
        ("Neg Risk CTF Exchange V2", neg_risk.exchange_v2),
        ("Neg Risk Adapter", neg_risk.neg_risk_adapter),
    ];
    for (name, spender) in spenders.into_iter().filter_map(|(n, a)| a.map(|a| (n, a))) {
        if collateral.allowance(owner, spender).call().await? < U256::MAX >> 1 {
            let tx = collateral.approve(spender, U256::MAX).send().await?.watch().await?;
            tracing::info!("pUSD approved for {name}: {tx}");
        }
        if !ctf.isApprovedForAll(owner, spender).call().await? {
            let tx = ctf.setApprovalForAll(spender, true).send().await?.watch().await?;
            tracing::info!("outcome tokens approved for {name}: {tx}");
        }
    }
    Ok(())
}
